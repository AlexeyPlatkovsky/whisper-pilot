use crate::error::{AppError, Result};
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

/// Structured notes generation output, domain-agnostic — the caller (Meeting
/// or Streaming) attaches its own id before persisting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedNotes {
    pub summary: String,
    pub decisions: String,
    pub action_items: String,
    pub open_questions: String,
    pub participants: String,
}

const ASSISTANT_PREFILL: &str = "{\"summary\":\"";

fn build_prompt(transcript: &str) -> String {
    let is_russian = transcript
        .chars()
        .any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));

    let (system_ru, user_ru) = if is_russian {
        (
            "Ты — ассистент для создания заметок о встречах. Заполни JSON-шаблон кратким содержанием из расшифровки.\n\
\n\
ПРАВИЛА:\n\
- summary: 2-3 предложения о ключевых темах обсуждения.\n\
- decisions: 2-3 предложения с принятыми решениями.\n\
- action_items: 2-3 предложения с дальнейшими шагами.\n\
- open_questions: 2-3 предложения с нерешёнными вопросами.\n\
- participants: имена через запятую или пустая строка.\n\
- Около 100-200 слов на все секции.\n\
- Экранируй двойные кавычки внутри значений как \\\".\n\
- Продолжи ТОЧНО с того места, где начинается ответ ассистента.",
            "Заполни JSON-шаблон.",
        )
    } else {
        (
            "You are a meeting notes assistant. Fill in the JSON template below with concise notes from the transcript.\n\
\n\
RULES:\n\
- summary: 2-3 sentences covering the key topics discussed.\n\
- decisions: 2-3 sentences listing conclusions reached.\n\
- action_items: 2-3 sentences with next steps.\n\
- open_questions: 2-3 sentences listing unanswered questions.\n\
- participants: comma-separated names, or empty string if none.\n\
- Target ~100-200 words total across all sections.\n\
- Escape any double-quotes inside values as backslash-quote.\n\
- Continue EXACTLY from where the assistant response starts below.",
            "Fill the JSON template.",
        )
    };

    format!(
        "<|im_start|>system\n\
{system_ru}<|im_end|>\n\
<|im_start|>user\n\
Transcript:\n{transcript}\n\n\
{user_ru}<|im_end|>\n\
<|im_start|>assistant\n\
<think>\n\n</think>\n\n\
{ASSISTANT_PREFILL}"
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

fn parse_notes_json(raw: &str) -> Result<GeneratedNotes> {
    let original = raw.trim();
    let cleaned = strip_think_block(original);
    let cleaned = cleaned.strip_prefix("```json").unwrap_or(cleaned);
    let cleaned = cleaned.strip_prefix('\n').unwrap_or(cleaned);
    let cleaned = cleaned.strip_prefix("```").unwrap_or(cleaned);

    let json_str = if cleaned.trim_start().starts_with('{') {
        cleaned.to_string()
    } else {
        format!("{ASSISTANT_PREFILL}{cleaned}")
    };

    let json_str = json_str.strip_suffix("```").unwrap_or(&json_str);
    let json_str = json_str.trim();

    if let Ok(parsed) = serde_json::from_str::<NotesJson>(json_str) {
        return Ok(GeneratedNotes {
            summary: parsed.summary,
            decisions: parsed.decisions,
            action_items: parsed.action_items,
            open_questions: parsed.open_questions,
            participants: parsed.participants,
        });
    }

    let fallback = NotesJson {
        summary: cleaned.to_string(),
        decisions: String::new(),
        action_items: String::new(),
        open_questions: String::new(),
        participants: String::new(),
    };

    Ok(GeneratedNotes {
        summary: fallback.summary,
        decisions: String::new(),
        action_items: String::new(),
        open_questions: String::new(),
        participants: String::new(),
    })
}

pub fn generate_notes(model_path: &Path, transcript: &str) -> Result<GeneratedNotes> {
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
        assert!(prompt.contains("meeting notes assistant"));
        assert!(prompt.ends_with(ASSISTANT_PREFILL));
    }

    #[test]
    fn parse_notes_prefills_opening_brace_when_missing() {
        let raw = "Test summary\", \"decisions\": \"D\", \"action_items\": \"A\", \"open_questions\": \"Q\", \"participants\": \"P\"}";
        let notes = parse_notes_json(raw).unwrap();
        assert_eq!(notes.summary, "Test summary");
        assert_eq!(notes.decisions, "D");
    }

    #[test]
    fn parse_notes_handles_full_json_when_model_regenerates_opening() {
        let raw = "{\"summary\": \"Test.\", \"decisions\": \"D\", \"action_items\": \"A\", \"open_questions\": \"Q\", \"participants\": \"P\"}";
        let notes = parse_notes_json(raw).unwrap();
        assert_eq!(notes.summary, "Test.");
    }

    #[test]
    fn build_prompt_uses_english_for_ascii_transcript() {
        let prompt = build_prompt("Hello, let us talk about Q3 roadmap.");
        assert!(prompt.contains("meeting notes assistant"));
        assert!(!prompt.contains("ассистент"));
    }

    #[test]
    fn build_prompt_uses_russian_for_cyrillic_transcript() {
        let prompt = build_prompt("Алексей: Привет\nИван: Здравствуйте");
        assert!(prompt.contains("ассистент"));
        assert!(prompt.contains("ПРАВИЛА"));
        assert!(!prompt.contains("meeting notes assistant"));
    }

    #[test]
    fn build_prompt_detects_cyrillic_even_when_mostly_ascii() {
        let prompt = build_prompt("We should обсуждать the budget.");
        assert!(prompt.contains("ассистент"));
    }
}
