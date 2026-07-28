use crate::error::{AppError, Result};
use crate::store::MeetingNotes;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use serde::Deserialize;
use std::num::NonZeroU32;
use std::path::Path;

const MAX_NEW_TOKENS: i32 = 1024;
const CTX_SIZE: u32 = 16384;
const N_BATCH: u32 = 2048;

#[derive(Debug, Deserialize)]
struct NotesJson {
    summary: String,
    decisions: String,
    #[serde(rename = "action_items")]
    action_items: String,
    #[serde(rename = "open_questions")]
    open_questions: String,
    participants: String,
}

fn build_prompt(transcript: &str) -> String {
    format!(
        "<|im_start|>system\n\
You are a meeting notes assistant. Given a full meeting transcript, produce structured notes in valid JSON with exactly these keys: summary, decisions, action_items, open_questions, participants. \
Each value is a plain string, never an array. Use newline characters within strings for lists. \
Keep each section concise. Respond in the same language as the transcript. Output ONLY the JSON object, no other text.<|im_end|>\n\
<|im_start|>user\n\
Transcript:\n{transcript}\n\n\
Produce the meeting notes JSON.<|im_end|>\n\
<|im_start|>assistant\n\
 response",
    )
}

fn strip_think_block(raw: &str) -> &str {
    if let Some(after_open) = raw.strip_prefix("<think>") {
        if let Some(idx) = after_open.find("</think>") {
            return after_open[idx + 8..].trim_start();
        }
        if let Some(idx) = after_open.find("\n\n") {
            return after_open[idx + 2..].trim_start();
        }
    }
    raw
}

fn parse_notes_json(raw: &str) -> Result<MeetingNotes> {
    let json_str = raw.trim();

    let json_str = strip_think_block(json_str);

    let json_str = json_str.strip_prefix("```json").unwrap_or(json_str);
    let json_str = json_str.strip_prefix('\n').unwrap_or(json_str);
    let json_str = json_str.strip_prefix("```").unwrap_or(json_str);
    let json_str = json_str.strip_suffix("```").unwrap_or(json_str);
    let json_str = json_str.trim();

    if let Ok(parsed) = serde_json::from_str::<NotesJson>(json_str) {
        return Ok(MeetingNotes {
            meeting_id: 0,
            summary: parsed.summary,
            decisions: parsed.decisions,
            action_items: parsed.action_items,
            open_questions: parsed.open_questions,
            participants: parsed.participants,
        });
    }

    let fallback = NotesJson {
        summary: json_str.to_string(),
        decisions: String::new(),
        action_items: String::new(),
        open_questions: String::new(),
        participants: String::new(),
    };

    Ok(MeetingNotes {
        meeting_id: 0,
        summary: fallback.summary,
        decisions: fallback.decisions,
        action_items: fallback.action_items,
        open_questions: fallback.open_questions,
        participants: fallback.participants,
    })
}

pub fn generate_notes(model_path: &Path, transcript: &str) -> Result<MeetingNotes> {
    let prompt = build_prompt(transcript);
    let raw_output = run_inference(model_path, &prompt)?;
    parse_notes_json(&raw_output)
}

fn run_inference(model_path: &Path, prompt: &str) -> Result<String> {
    let model_path = model_path.to_path_buf();
    let prompt = prompt.to_string();

    let backend = LlamaBackend::init().map_err(|e| AppError::Llm(e.to_string()))?;

    let model_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
        .map_err(|e| AppError::Llm(format!("model load: {e}")))?;

    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(4);
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(CTX_SIZE))
        .with_n_batch(N_BATCH)
        .with_n_threads(n_threads);

    let mut ctx = model
        .new_context(&backend, ctx_params)
        .map_err(|e| AppError::Llm(format!("context create: {e}")))?;

    let tokens_list = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|e| AppError::Llm(format!("tokenize: {e}")))?;

    let total = tokens_list.len();
    let chunk_size = N_BATCH as usize;
    let mut n_cur: i32 = 0;
    let mut batch = LlamaBatch::new(chunk_size, 1);

    for chunk_start in (0..total).step_by(chunk_size) {
        let chunk_end = (chunk_start + chunk_size).min(total);
        let is_last_chunk = chunk_end == total;

        batch.clear();
        for (offset, &token) in tokens_list[chunk_start..chunk_end].iter().enumerate() {
            let pos = n_cur + offset as i32;
            let logits = is_last_chunk && offset == chunk_end - chunk_start - 1;
            batch
                .add(token, pos, &[0], logits)
                .map_err(|e| AppError::Llm(format!("batch add: {e}")))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| AppError::Llm(format!("decode: {e}")))?;
        n_cur += (chunk_end - chunk_start) as i32;
    }

    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut sampler = LlamaSampler::greedy();
    let mut output = String::new();
    let eos_token = model.token_eos();

    while n_cur - total as i32 <= MAX_NEW_TOKENS {
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);

        sampler.accept(token);

        if token == eos_token {
            break;
        }

        match model.token_to_piece(token, &mut decoder, true, None) {
            Ok(piece) => output.push_str(&piece),
            Err(e) => return Err(AppError::Llm(format!("token to piece: {e}"))),
        }

        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|e| AppError::Llm(format!("batch add: {e}")))?;

        n_cur += 1;

        ctx.decode(&mut batch)
            .map_err(|e| AppError::Llm(format!("decode: {e}")))?;
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_notes_accepts_valid_json() {
        let raw = r#"{"summary": "Discussed Q3 roadmap.", "decisions": "Ship M1 by Friday.", "action_items": "Alex: update deck", "open_questions": "Budget for Q4?", "participants": "Alex, Sam"}"#;
        let notes = parse_notes_json(raw).unwrap();
        assert_eq!(notes.summary, "Discussed Q3 roadmap.");
        assert_eq!(notes.decisions, "Ship M1 by Friday.");
        assert_eq!(notes.action_items, "Alex: update deck");
        assert_eq!(notes.open_questions, "Budget for Q4?");
        assert_eq!(notes.participants, "Alex, Sam");
    }

    #[test]
    fn parse_notes_strips_markdown_fences() {
        let raw = "```json\n{\"summary\": \"Test.\", \"decisions\": \"\", \"action_items\": \"\", \"open_questions\": \"\", \"participants\": \"\"}\n```";
        let notes = parse_notes_json(raw).unwrap();
        assert_eq!(notes.summary, "Test.");
    }

    #[test]
    fn parse_notes_falls_back_to_raw_text() {
        let raw = "Not valid JSON at all.";
        let notes = parse_notes_json(raw).unwrap();
        assert_eq!(notes.summary, raw);
        assert!(notes.decisions.is_empty());
    }

    #[test]
    fn parse_notes_strips_think_block() {
        let raw = "<think>\nLet me think about this...\n</think>\n{\"summary\": \"Test.\", \"decisions\": \"\", \"action_items\": \"\", \"open_questions\": \"\", \"participants\": \"\"}";
        let notes = parse_notes_json(raw).unwrap();
        assert_eq!(notes.summary, "Test.");
    }

    #[test]
    fn parse_notes_strips_think_without_closing_tag() {
        let raw = "<think>\nreasoning here\n\n{\"summary\": \"Done.\", \"decisions\": \"\", \"action_items\": \"\", \"open_questions\": \"\", \"participants\": \"\"}";
        let notes = parse_notes_json(raw).unwrap();
        assert_eq!(notes.summary, "Done.");
    }

    #[test]
    fn build_prompt_includes_transcript() {
        let transcript = "Alex: Hello\nSam: Hi there";
        let prompt = build_prompt(transcript);
        assert!(prompt.contains("Alex: Hello"));
        assert!(prompt.contains("You are a meeting notes assistant"));
        assert!(prompt.contains("<|im_start|>assistant\n"));
    }
}
