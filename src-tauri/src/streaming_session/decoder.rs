//! Model-specific implementations of the Streaming decoder seam.

use super::*;

/// The production [`SessionDecoder`]: owns the session's single
/// `WhisperState`, created once via [`WhisperSessionDecoder::new`] and reused
/// for every window.
pub struct WhisperSessionDecoder {
    state: WhisperState,
}

impl WhisperSessionDecoder {
    pub fn new(ctx: &WhisperContext) -> crate::error::Result<Self> {
        let state = ctx
            .create_state()
            .map_err(|e| AppError::Transcribe(e.to_string()))?;
        Ok(Self { state })
    }
}

impl SessionDecoder for WhisperSessionDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        context: Option<&str>,
        profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        let prompt = whisper_streaming_prompt(context);
        transcribe::transcribe_with_state_and_prompt_profile(
            &mut self.state,
            samples,
            Some(&prompt),
            profile,
        )
    }

    fn has_reliable_segment_timestamps(&self) -> bool {
        true
    }
}

pub(super) fn whisper_streaming_prompt(context: Option<&str>) -> String {
    match context.filter(|value| !value.trim().is_empty()) {
        Some(context) => format!("{WHISPER_PUNCTUATION_SEED} {}", context.trim()),
        None => WHISPER_PUNCTUATION_SEED.to_string(),
    }
}

/// Window decoder for the GGUF Qwen3-ASR runtime. llama.cpp/MTMD owns audio
/// encoding and language detection; the surrounding window supplies stable
/// session-relative timestamps.
#[cfg(target_os = "macos")]
pub struct QwenGgufSessionDecoder {
    model: std::sync::Arc<crate::qwen_gguf_asr::QwenGgufAsrModel>,
}

#[cfg(target_os = "macos")]
impl QwenGgufSessionDecoder {
    pub fn new(model: std::sync::Arc<crate::qwen_gguf_asr::QwenGgufAsrModel>) -> Self {
        Self { model }
    }
}

#[cfg(target_os = "macos")]
impl SessionDecoder for QwenGgufSessionDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        context: Option<&str>,
        _profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        self.model.transcribe_window_with_context(samples, context)
    }
}
