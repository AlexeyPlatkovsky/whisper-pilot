//! Qwen3-ASR GGUF inference through llama.cpp's MTMD audio path.

use crate::error::{AppError, Result};
use crate::llm::SharedLlamaBackend;
use crate::transcribe::{Segment, Transcription};
use encoding_rs::UTF_8;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
use llama_cpp_2::mtmd::{
    mtmd_default_marker, MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText,
};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;

const CONTEXT_TOKENS: u32 = 8_192;
const BATCH_TOKENS: u32 = 2_048;
const MAX_OUTPUT_TOKENS: i32 = 2_048;

/// Loaded text backbone plus audio projector. A decoder creates a fresh KV
/// context per bounded audio window; model weights and Metal allocations stay
/// cached for the selected ASR model.
pub struct QwenGgufAsrModel {
    mtmd: MtmdContext,
    model: LlamaModel,
    backend: Arc<SharedLlamaBackend>,
}

impl QwenGgufAsrModel {
    pub(crate) fn load(
        model_path: &Path,
        mmproj_path: &Path,
        backend: Arc<SharedLlamaBackend>,
    ) -> Result<Self> {
        let model =
            LlamaModel::load_from_file(backend.get()?, model_path, &LlamaModelParams::default())
                .map_err(|error| AppError::ModelLoad(format!("Qwen3-ASR GGUF model: {error}")))?;
        let mmproj = mmproj_path.to_str().ok_or_else(|| {
            AppError::ModelLoad("Qwen3-ASR projector path is not valid UTF-8".into())
        })?;
        let params = MtmdContextParams {
            use_gpu: true,
            print_timings: false,
            n_threads: available_threads(),
            ..MtmdContextParams::default()
        };
        let mtmd = MtmdContext::init_from_file(mmproj, &model, &params)
            .map_err(|error| AppError::ModelLoad(format!("Qwen3-ASR audio projector: {error}")))?;
        if !mtmd.support_audio() {
            return Err(AppError::ModelLoad(
                "Qwen3-ASR projector does not expose audio input".into(),
            ));
        }
        if mtmd.get_audio_sample_rate() != Some(crate::audio::SAMPLE_RATE) {
            return Err(AppError::ModelLoad(format!(
                "Qwen3-ASR projector requires {:?} Hz instead of {} Hz",
                mtmd.get_audio_sample_rate(),
                crate::audio::SAMPLE_RATE
            )));
        }
        Ok(Self {
            mtmd,
            model,
            backend,
        })
    }

    pub(crate) fn transcribe_window_with_context(
        &self,
        samples: &[f32],
        context: Option<&str>,
    ) -> Result<Transcription> {
        let bitmap = MtmdBitmap::from_audio_data(samples)
            .map_err(|error| AppError::Transcribe(format!("Qwen3-ASR audio input: {error}")))?;
        let template = self
            .model
            .chat_template(None)
            .map_err(|error| AppError::Transcribe(format!("Qwen3-ASR chat template: {error}")))?;
        let content = format!("{}Transcribe this audio exactly.", mtmd_default_marker());
        let message = LlamaChatMessage::new("user".into(), content)
            .map_err(|error| AppError::Transcribe(format!("Qwen3-ASR prompt: {error}")))?;
        let mut messages = Vec::with_capacity(2);
        if let Some(context) = context.filter(|context| !context.trim().is_empty()) {
            messages.push(
                LlamaChatMessage::new("system".into(), context_instruction(context)).map_err(
                    |error| AppError::Transcribe(format!("Qwen3-ASR context prompt: {error}")),
                )?,
            );
        }
        messages.push(message);
        let prompt = self
            .model
            .apply_chat_template(&template, &messages, true)
            .map_err(|error| AppError::Transcribe(format!("Qwen3-ASR prompt template: {error}")))?;
        let chunks = self
            .mtmd
            .tokenize(
                MtmdInputText {
                    text: prompt,
                    add_special: true,
                    parse_special: true,
                },
                &[&bitmap],
            )
            .map_err(|error| AppError::Transcribe(format!("Qwen3-ASR tokenize: {error}")))?;

        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(CONTEXT_TOKENS))
            .with_n_batch(BATCH_TOKENS)
            .with_n_threads(available_threads());
        let mut context = self
            .model
            .new_context(self.backend.get()?, params)
            .map_err(|error| AppError::Transcribe(format!("Qwen3-ASR context: {error}")))?;
        let initial_n_past = chunks
            .eval_chunks(&self.mtmd, &context, 0, 0, BATCH_TOKENS as i32, true)
            .map_err(|error| AppError::Transcribe(format!("Qwen3-ASR audio decode: {error}")))?;

        let mut sampler = LlamaSampler::greedy();
        let mut decoder = UTF_8.new_decoder();
        let mut output = String::new();
        let mut batch = LlamaBatch::new(1, 1);
        for generated in 0..MAX_OUTPUT_TOKENS {
            let token = sampler.sample(&context, -1);
            sampler.accept(token);
            if self.model.is_eog_token(token) {
                break;
            }
            let piece = self
                .model
                .token_to_piece(token, &mut decoder, true, None)
                .map_err(|error| {
                    AppError::Transcribe(format!("Qwen3-ASR output token: {error}"))
                })?;
            output.push_str(&piece);
            if output.contains("<|im_end|>") || output.contains("<|endoftext|>") {
                break;
            }
            batch.clear();
            batch
                .add(token, initial_n_past + generated, &[0], true)
                .map_err(|error| AppError::Transcribe(format!("Qwen3-ASR batch: {error}")))?;
            context
                .decode(&mut batch)
                .map_err(|error| AppError::Transcribe(format!("Qwen3-ASR decode: {error}")))?;
        }

        Ok(protocol_output_to_transcription(&output, samples.len()))
    }
}

fn context_instruction(context: &str) -> String {
    format!(
        "Previous confirmed transcript, provided only to preserve vocabulary and continuity:\n\
         <previous_transcript>\n{}\n</previous_transcript>\n\
         Do not repeat or rewrite the previous transcript. Output only speech from the current audio.",
        context.trim()
    )
}

fn available_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|threads| threads.get() as i32)
        .unwrap_or(4)
}

fn protocol_output_to_transcription(output: &str, sample_count: usize) -> Transcription {
    let output = output.trim();
    let (language, text) = output
        .strip_prefix("language ")
        .and_then(|rest| rest.split_once("<asr_text>"))
        .map(|(language, text)| (normalize_language_code(language), text))
        .unwrap_or_else(|| ("auto".to_string(), output));
    let stop = ["<|im_end|>", "<|endoftext|>"]
        .iter()
        .filter_map(|token| text.find(token))
        .min()
        .unwrap_or(text.len());
    let text = text[..stop].trim();
    let segments = (!text.is_empty())
        .then(|| Segment {
            start_ms: 0,
            end_ms: sample_count as u64 * 1_000 / crate::audio::SAMPLE_RATE as u64,
            text: text.to_string(),
            speaker_id: None,
        })
        .into_iter()
        .collect();
    Transcription { segments, language }
}

fn normalize_language_code(language: &str) -> String {
    match language.trim().to_ascii_lowercase().as_str() {
        "english" => "en".to_string(),
        "russian" => "ru".to_string(),
        "turkish" => "tr".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{context_instruction, protocol_output_to_transcription, QwenGgufAsrModel};

    #[test]
    fn previous_transcript_is_labeled_as_context_that_must_not_be_repeated() {
        let prompt = context_instruction("предыдущая подтвержденная фраза");

        assert!(prompt.contains("Previous confirmed transcript"));
        assert!(prompt.contains("Do not repeat"));
        assert!(prompt.contains("предыдущая подтвержденная фраза"));
    }

    #[test]
    fn strips_qwen_asr_protocol_metadata_from_visible_transcript() {
        let result = protocol_output_to_transcription(
            "language Russian<asr_text>Привет, world.<|im_end|>",
            16_000,
        );
        assert_eq!(result.language, "ru");
        assert_eq!(result.segments[0].text, "Привет, world.");
        assert_eq!(result.segments[0].end_ms, 1_000);
    }

    #[test]
    fn real_qwen_gguf_audio_smoke_when_fixture_paths_are_available() {
        let Ok(model_path) = std::env::var("QWEN3_ASR_GGUF_MODEL") else {
            eprintln!("SKIP: set QWEN3_ASR_GGUF_MODEL for the real GGUF ASR gate");
            return;
        };
        let Ok(mmproj_path) = std::env::var("QWEN3_ASR_GGUF_MMPROJ") else {
            eprintln!("SKIP: set QWEN3_ASR_GGUF_MMPROJ for the real GGUF ASR gate");
            return;
        };
        let Ok(audio_path) = std::env::var("QWEN3_ASR_GGUF_TEST_AUDIO") else {
            eprintln!("SKIP: set QWEN3_ASR_GGUF_TEST_AUDIO for the real GGUF ASR gate");
            return;
        };
        let backend = std::sync::Arc::new(crate::llm::SharedLlamaBackend::default());
        let model = QwenGgufAsrModel::load(
            std::path::Path::new(&model_path),
            std::path::Path::new(&mmproj_path),
            backend,
        )
        .expect("load Qwen3-ASR GGUF bundle");
        let samples = crate::audio::load_samples(std::path::Path::new(&audio_path))
            .expect("decode real ASR fixture");
        let transcription = model
            .transcribe_window_with_context(
                &samples,
                Some("Whisper Pilot previously confirmed a separate sentence."),
            )
            .expect("transcribe real ASR fixture");
        let text = transcription
            .segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !text.trim().is_empty(),
            "real Qwen3-ASR GGUF decode must emit text"
        );
        if let Ok(expected) = std::env::var("QWEN3_ASR_GGUF_EXPECTED_TERMS") {
            let normalized = text.to_lowercase();
            for term in expected
                .split(',')
                .map(str::trim)
                .filter(|term| !term.is_empty())
            {
                assert!(
                    normalized.contains(&term.to_lowercase()),
                    "real Qwen3-ASR transcript must contain expected term {term:?}"
                );
            }
        }
        if let Ok(forbidden) = std::env::var("QWEN3_ASR_GGUF_FORBIDDEN_TERMS") {
            let normalized = text.to_lowercase();
            for term in forbidden
                .split(',')
                .map(str::trim)
                .filter(|term| !term.is_empty())
            {
                assert!(
                    !normalized.contains(&term.to_lowercase()),
                    "real Qwen3-ASR transcript repeated forbidden context term {term:?}"
                );
            }
        }
    }
}
