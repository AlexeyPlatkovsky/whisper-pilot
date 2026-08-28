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
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_NEW_TOKENS: i32 = 1024;
const CTX_SIZE: u32 = 16384;
const N_BATCH: u32 = 2048;

#[derive(Debug, Deserialize)]
struct MfuJson {
    summary: String,
    decisions: String,
    #[serde(rename = "action_items")]
    action_items: String,
    #[serde(rename = "open_questions")]
    open_questions: String,
    participants: String,
}

/// Structured mfu generation output, domain-agnostic — the caller (Meeting
/// or Streaming) attaches its own id before persisting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedMfu {
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
            "You are a meeting mfu assistant. Fill in the JSON template below with concise mfu from the transcript.\n\
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

fn parse_notes_json(raw: &str) -> Result<GeneratedMfu> {
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

    if let Ok(parsed) = serde_json::from_str::<MfuJson>(json_str) {
        return Ok(GeneratedMfu {
            summary: parsed.summary,
            decisions: parsed.decisions,
            action_items: parsed.action_items,
            open_questions: parsed.open_questions,
            participants: parsed.participants,
        });
    }

    let fallback = MfuJson {
        summary: cleaned.to_string(),
        decisions: String::new(),
        action_items: String::new(),
        open_questions: String::new(),
        participants: String::new(),
    };

    Ok(GeneratedMfu {
        summary: fallback.summary,
        decisions: String::new(),
        action_items: String::new(),
        open_questions: String::new(),
        participants: String::new(),
    })
}

pub fn generate_mfu(model_path: &Path, transcript: &str) -> Result<GeneratedMfu> {
    let prompt = build_prompt(transcript);
    let raw_output = run_inference(model_path, &prompt)?;
    parse_notes_json(&raw_output)
}

fn build_prettify_prompt(transcript: &str) -> String {
    let is_russian = transcript
        .chars()
        .any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));

    let (system, user) = if is_russian {
        (
            "Ты — ассистент для осторожной очистки расшифровок. Отредактируй текст, сохраняя его содержание:\n\
убери только очевидные слова-паразиты и соседние повторы, а грамматику исправляй минимально.\n\
\n\
ПРАВИЛА:\n\
- Не удаляй законченные предложения, факты, имена, числа, технические термины или фрагменты на другом языке.\n\
- Сохрани исходный смысл, порядок мыслей и все языковые переключения — не добавляй новую информацию.\n\
- Если не уверен, оставь исходный фрагмент без изменений.\n\
- Верни только очищенный текст, без пояснений и без разметки.",
            "Очисти расшифровку.",
        )
    } else {
        (
            "You are a conservative transcript cleanup assistant. Edit the transcript while preserving its content:\n\
remove only obvious filler words and adjacent repetitions, and make minimal grammar fixes.\n\
\n\
RULES:\n\
- Do not delete complete sentences, facts, names, numbers, technical terms, or passages in another language.\n\
- Preserve the original meaning, order of ideas, and every language switch — do not add new information.\n\
- If unsure, copy the original fragment unchanged.\n\
- Return only the cleaned text, with no explanation and no markup.",
            "Clean up the transcript.",
        )
    };

    format!(
        "<|im_start|>system\n\
{system}<|im_end|>\n\
<|im_start|>user\n\
Transcript:\n{transcript}\n\n\
{user}<|im_end|>\n\
<|im_start|>assistant\n\
<think>\n\n</think>\n\n"
    )
}

/// Strips a `<think>` reasoning block and markdown fences the model may wrap
/// its output in — the same class of cleanup `parse_notes_json` already does
/// for the JSON path, applied here to plain text instead.
fn clean_prettify_output(raw: &str) -> String {
    let cleaned = strip_think_block(raw.trim());
    let cleaned = cleaned.strip_prefix("```").unwrap_or(cleaned);
    let cleaned = cleaned.strip_suffix("```").unwrap_or(cleaned);
    cleaned.trim().to_string()
}

pub fn prettify_transcript(model_path: &Path, transcript: &str) -> Result<String> {
    let prompt = build_prettify_prompt(transcript);
    let raw_output = run_inference(model_path, &prompt)?;
    let cleaned = clean_prettify_output(&raw_output);
    validate_prettify_candidate(transcript, &cleaned)
}

fn protected_tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace().filter(|token| {
        let token = normalize_protected_token(token);
        let has_digit = token.chars().any(|c| c.is_ascii_digit());
        let has_separator = token.contains('-') || token.contains('_');
        let uppercase_count = token.chars().filter(|c| c.is_uppercase()).count();
        !token.is_empty() && (has_digit || has_separator || uppercase_count >= 2)
    })
}

fn normalize_protected_token(token: &str) -> &str {
    token
        .trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '%' && c != '-' && c != '_' && c != '.'
        })
        .trim_end_matches('.')
}

fn contains_cyrillic(text: &str) -> bool {
    text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

fn contains_latin(text: &str) -> bool {
    text.chars().any(|c| ('\u{0041}'..='\u{024F}').contains(&c))
}

fn validate_prettify_candidate(original: &str, candidate: &str) -> Result<String> {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return Err(AppError::Llm(
            "prettify returned an empty transcript; the raw transcript was kept".into(),
        ));
    }

    let original_words = original.split_whitespace().count();
    let candidate_words = candidate.split_whitespace().count();
    if original_words > 0 && candidate_words * 2 < original_words {
        return Err(AppError::Llm(
            "prettify suggestion removed too much transcript content; review was rejected".into(),
        ));
    }
    if candidate_words > original_words.saturating_mul(3) / 2 + 8 {
        return Err(AppError::Llm(
            "prettify suggestion added too much new content; review was rejected".into(),
        ));
    }

    if contains_cyrillic(original) && !contains_cyrillic(candidate) {
        return Err(AppError::Llm(
            "prettify suggestion dropped the original Cyrillic content; review was rejected".into(),
        ));
    }
    if contains_latin(original) && !contains_latin(candidate) {
        return Err(AppError::Llm(
            "prettify suggestion dropped the original Latin content; review was rejected".into(),
        ));
    }

    for token in protected_tokens(original) {
        let normalized = normalize_protected_token(token);
        let retained = candidate
            .split_whitespace()
            .map(normalize_protected_token)
            .any(|candidate_token| candidate_token == normalized);
        if !retained {
            return Err(AppError::Llm(format!(
                "prettify suggestion dropped protected term '{normalized}'; review was rejected"
            )));
        }
    }

    Ok(candidate.to_string())
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

/// Streaming window translation targets (WP-92; WP-103 moved the
/// translation unit from a whole paragraph to a single 7s window): local-only,
/// via the active summary LLM. Source language is Streaming's own per-window
/// auto-detection and is never an input here.
pub const SUPPORTED_TRANSLATION_TARGETS: [&str; 2] = ["en", "ru"];

pub fn is_supported_target_language(target_language: &str) -> bool {
    SUPPORTED_TRANSLATION_TARGETS.contains(&target_language)
}

/// Which alphabet a piece of text is written in, at the coarseness this
/// feature's supported targets need — just enough to tell "the model
/// actually translated" from "the model echoed the source back".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Cyrillic,
    Latin,
    Other,
}

/// The script whose character count is highest wins; `Other` only when
/// neither script appears at all (e.g. digits-only text), not on a tie.
fn dominant_script(text: &str) -> Script {
    let mut cyrillic = 0usize;
    let mut latin = 0usize;
    for c in text.chars() {
        if ('\u{0400}'..='\u{04FF}').contains(&c) {
            cyrillic += 1;
        } else if c.is_ascii_alphabetic() || ('\u{00C0}'..='\u{024F}').contains(&c) {
            latin += 1;
        }
    }
    if cyrillic == 0 && latin == 0 {
        Script::Other
    } else if cyrillic > latin {
        Script::Cyrillic
    } else {
        Script::Latin
    }
}

fn expected_script_for_target(target_language: &str) -> Script {
    if target_language == "ru" {
        Script::Cyrillic
    } else {
        Script::Latin
    }
}

/// `prior_context`, when `Some`, is the immediately preceding window's (or,
/// pre-WP-103, paragraph's) already-translated text (WP-100) — included as
/// reference-only context ahead of the actual text to translate, with an
/// explicit instruction (in the same language as the rest of the prompt)
/// not to repeat or re-translate it. `None` produces byte-identical output
/// to the pre-WP-100 source-only prompt.
fn build_translate_prompt(
    source_text: &str,
    target_language: &str,
    prior_context: Option<&str>,
) -> String {
    let (system, user, context_intro) = if target_language == "ru" {
        (
            "Ты — ассистент-переводчик. Переведи текст ниже на русский язык точно и полностью, сохраняя смысл, тон, факты, числа и имена.\n\
\n\
ПРАВИЛА:\n\
- Переведи весь текст целиком, не сокращай, не суммируй и не пересказывай.\n\
- Не добавляй пояснений, комментариев и не отвечай на вопросы из текста.\n\
- Не добавляй информацию, которой нет в оригинале.\n\
- Верни только переведённый текст, без разметки и кавычек.",
            "Переведи текст.",
            "Контекст предыдущего абзаца (только для справки — НЕ переводи и не повторяй его в ответе; переведи только текст ниже):",
        )
    } else {
        (
            "You are a translation assistant. Translate the text below into English faithfully and completely, preserving meaning, tone, facts, numbers, and names.\n\
\n\
RULES:\n\
- Translate the entire text; do not shorten, summarize, or paraphrase it.\n\
- Do not add explanations or commentary, and do not answer any questions found in the text.\n\
- Do not add information that is not present in the original.\n\
- Return only the translated text, with no markup or quotation marks.",
            "Translate the text.",
            "Context from the previous paragraph (for reference only — do NOT translate or repeat it in your answer; translate only the text below):",
        )
    };

    let context_block = match prior_context {
        Some(ctx) => format!("{context_intro}\n{ctx}\n\n"),
        None => String::new(),
    };

    format!(
        "<|im_start|>system\n\
{system}<|im_end|>\n\
<|im_start|>user\n\
{context_block}Text:\n{source_text}\n\n\
{user}<|im_end|>\n\
<|im_start|>assistant\n\
<think>\n\n</think>\n\n"
    )
}

/// Prompt-template overhead reserved on top of the estimated source and
/// generated-output tokens when checking `CTX_SIZE` — generous headroom for
/// the system/user scaffolding `build_translate_prompt` adds.
const TRANSLATE_PROMPT_OVERHEAD_TOKENS: usize = 200;

/// Chars-per-token used only to reject a window (or, pre-WP-103, paragraph)
/// that would clearly overflow `CTX_SIZE` before spending an inference call
/// on it — the real tokenizer, loaded lazily inside `run_inference`, is what
/// actually enforces the hard limit. Script-dependent: Cyrillic tokenizes
/// denser than Latin under Qwen/ChatML-style tokenizers.
const LATIN_CHARS_PER_TOKEN: usize = 4;
const CYRILLIC_CHARS_PER_TOKEN: usize = 2;

fn estimated_token_count(text: &str) -> usize {
    let chars_per_token = match dominant_script(text) {
        Script::Cyrillic => CYRILLIC_CHARS_PER_TOKEN,
        Script::Latin | Script::Other => LATIN_CHARS_PER_TOKEN,
    };
    text.chars().count() / chars_per_token + 1
}

/// `prior_context`'s estimated length is added on top of `source_text`'s
/// (WP-100) — reusing `estimated_token_count`'s own script-aware estimate
/// for each rather than adding a second detection path — so a combined
/// prompt that would overflow `CTX_SIZE` is rejected here, before an
/// inference call is spent on it, exactly as a source-only overflow already
/// was.
fn ensure_translation_fits_context_budget(
    source_text: &str,
    prior_context: Option<&str>,
) -> Result<()> {
    let estimated = estimated_token_count(source_text)
        + prior_context.map_or(0, estimated_token_count)
        + TRANSLATE_PROMPT_OVERHEAD_TOKENS
        + MAX_NEW_TOKENS as usize;
    if estimated > CTX_SIZE as usize {
        return Err(AppError::Llm(
            "paragraph is too long to translate within the model's context window; split it into smaller paragraphs".into(),
        ));
    }
    Ok(())
}

fn validate_translation_candidate(
    source: &str,
    candidate: &str,
    target_language: &str,
) -> Result<String> {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return Err(AppError::Llm(
            "translation returned an empty result; the paragraph was not translated".into(),
        ));
    }

    let source_words = source.split_whitespace().count();
    let candidate_words = candidate.split_whitespace().count();
    if source_words > 0 && candidate_words * 3 < source_words {
        return Err(AppError::Llm(
            "translation is much shorter than the source paragraph; review was rejected".into(),
        ));
    }
    if candidate_words > source_words.saturating_mul(3) + 10 {
        return Err(AppError::Llm(
            "translation is much longer than the source paragraph; review was rejected".into(),
        ));
    }

    let source_script = dominant_script(source);
    let target_script = expected_script_for_target(target_language);
    if source_script != Script::Other
        && source_script != target_script
        && dominant_script(candidate) == source_script
    {
        return Err(AppError::Llm(
            "translation is still predominantly in the source language; review was rejected".into(),
        ));
    }

    Ok(candidate.to_string())
}

/// Translates one Streaming translation unit — a single window as of
/// WP-103, previously a whole paragraph — into `target_language` ("en" or
/// "ru") using the same llama.cpp path `prettify_transcript` uses. Callers
/// validate `target_language`/`source_text` first; this still guards the
/// model's context budget and never silently truncates. `prior_context`,
/// when `Some`, is reference-only context, not itself re-translated.
/// Signature/name unchanged by WP-103 — only what a caller assembles as
/// `source_text`/`prior_context` changed.
pub fn translate_paragraph(
    model_path: &Path,
    source_text: &str,
    target_language: &str,
    prior_context: Option<&str>,
) -> Result<String> {
    ensure_translation_fits_context_budget(source_text, prior_context)?;
    let prompt = build_translate_prompt(source_text, target_language, prior_context);
    let raw_output = run_inference(model_path, &prompt)?;
    let cleaned = clean_prettify_output(&raw_output);
    validate_translation_candidate(source_text, &cleaned, target_language)
}

/// RAII hold enforcing translation single-flight (WP-92): at most one
/// translation runs at a time, isolated from the streaming decode loop's own
/// (separate) Whisper-model transcription. Mirrors
/// `streaming_session::WhisperUsageGuard`'s claim/release idiom — a
/// compare-exchange over a shared atomic, released on drop — applied to the
/// LLM's own contention point. Non-blocking single-flight guard; contention
/// returns `AppError::TranslationBusy` to the caller.
pub struct TranslationUsageGuard<'a> {
    busy: &'a AtomicBool,
}

impl<'a> TranslationUsageGuard<'a> {
    /// Contention returns `Err(())`, which the caller
    /// (`translate_streaming_window`) maps to `AppError::TranslationBusy`.
    #[allow(clippy::result_unit_err)]
    pub fn acquire(busy: &'a AtomicBool) -> std::result::Result<Self, ()> {
        match busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => Ok(Self { busy }),
            Err(_) => Err(()),
        }
    }
}

impl Drop for TranslationUsageGuard<'_> {
    fn drop(&mut self) {
        self.busy.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_notes_accepts_valid_json() {
        let raw = r#"{"summary": "Discussed Q3 roadmap.", "decisions": "Ship M1 by Friday.", "action_items": "Alex: update deck", "open_questions": "Budget for Q4?", "participants": "Alex, Sam"}"#;
        let mfu = parse_notes_json(raw).unwrap();
        assert_eq!(mfu.summary, "Discussed Q3 roadmap.");
        assert_eq!(mfu.decisions, "Ship M1 by Friday.");
        assert_eq!(mfu.action_items, "Alex: update deck");
        assert_eq!(mfu.open_questions, "Budget for Q4?");
        assert_eq!(mfu.participants, "Alex, Sam");
    }

    #[test]
    fn parse_notes_strips_markdown_fences() {
        let raw = "```json\n{\"summary\": \"Test.\", \"decisions\": \"\", \"action_items\": \"\", \"open_questions\": \"\", \"participants\": \"\"}\n```";
        let mfu = parse_notes_json(raw).unwrap();
        assert_eq!(mfu.summary, "Test.");
    }

    #[test]
    fn parse_notes_falls_back_to_raw_text() {
        let raw = "Not valid JSON at all.";
        let mfu = parse_notes_json(raw).unwrap();
        assert_eq!(mfu.summary, raw);
        assert!(mfu.decisions.is_empty());
    }

    #[test]
    fn parse_notes_strips_think_block() {
        let raw = "<think>\nLet me think about this...\n</think>\n{\"summary\": \"Test.\", \"decisions\": \"\", \"action_items\": \"\", \"open_questions\": \"\", \"participants\": \"\"}";
        let mfu = parse_notes_json(raw).unwrap();
        assert_eq!(mfu.summary, "Test.");
    }

    #[test]
    fn parse_notes_strips_think_without_closing_tag() {
        let raw = "<think>\nreasoning here\n\n{\"summary\": \"Done.\", \"decisions\": \"\", \"action_items\": \"\", \"open_questions\": \"\", \"participants\": \"\"}";
        let mfu = parse_notes_json(raw).unwrap();
        assert_eq!(mfu.summary, "Done.");
    }

    #[test]
    fn build_prompt_includes_transcript() {
        let transcript = "Alex: Hello\nSam: Hi there";
        let prompt = build_prompt(transcript);
        assert!(prompt.contains("Alex: Hello"));
        assert!(prompt.contains("meeting mfu assistant"));
        assert!(prompt.ends_with(ASSISTANT_PREFILL));
    }

    #[test]
    fn parse_notes_prefills_opening_brace_when_missing() {
        let raw = "Test summary\", \"decisions\": \"D\", \"action_items\": \"A\", \"open_questions\": \"Q\", \"participants\": \"P\"}";
        let mfu = parse_notes_json(raw).unwrap();
        assert_eq!(mfu.summary, "Test summary");
        assert_eq!(mfu.decisions, "D");
    }

    #[test]
    fn parse_notes_handles_full_json_when_model_regenerates_opening() {
        let raw = "{\"summary\": \"Test.\", \"decisions\": \"D\", \"action_items\": \"A\", \"open_questions\": \"Q\", \"participants\": \"P\"}";
        let mfu = parse_notes_json(raw).unwrap();
        assert_eq!(mfu.summary, "Test.");
    }

    #[test]
    fn build_prompt_uses_english_for_ascii_transcript() {
        let prompt = build_prompt("Hello, let us talk about Q3 roadmap.");
        assert!(prompt.contains("meeting mfu assistant"));
        assert!(!prompt.contains("ассистент"));
    }

    #[test]
    fn build_prompt_uses_russian_for_cyrillic_transcript() {
        let prompt = build_prompt("Алексей: Привет\nИван: Здравствуйте");
        assert!(prompt.contains("ассистент"));
        assert!(prompt.contains("ПРАВИЛА"));
        assert!(!prompt.contains("meeting mfu assistant"));
    }

    #[test]
    fn build_prompt_detects_cyrillic_even_when_mostly_ascii() {
        let prompt = build_prompt("We should обсуждать the budget.");
        assert!(prompt.contains("ассистент"));
    }

    #[test]
    fn build_prettify_prompt_includes_transcript() {
        let prompt = build_prettify_prompt("Um so like we should, you know, ship it.");
        assert!(prompt.contains("Um so like we should, you know, ship it."));
        assert!(prompt.contains("cleanup"));
    }

    #[test]
    fn build_prettify_prompt_uses_english_for_ascii_transcript() {
        let prompt = build_prettify_prompt("Let's talk about the roadmap.");
        assert!(prompt.contains("cleanup"));
        assert!(!prompt.contains("ассистент"));
    }

    #[test]
    fn build_prettify_prompt_uses_russian_for_cyrillic_transcript() {
        let prompt = build_prettify_prompt("Привет, давайте обсудим план.");
        assert!(prompt.contains("ассистент"));
        assert!(!prompt.contains("cleanup"));
    }

    #[test]
    fn clean_prettify_output_passes_through_plain_text_unchanged() {
        assert_eq!(
            clean_prettify_output("Let's kick off the meeting."),
            "Let's kick off the meeting."
        );
    }

    #[test]
    fn clean_prettify_output_strips_think_block() {
        let raw = "<think>\nreasoning about cleanup\n</think>\nCleaned text here.";
        assert_eq!(clean_prettify_output(raw), "Cleaned text here.");
    }

    #[test]
    fn clean_prettify_output_strips_markdown_fences() {
        assert_eq!(
            clean_prettify_output("```\nCleaned text here.\n```"),
            "Cleaned text here."
        );
    }

    #[test]
    fn clean_prettify_output_trims_surrounding_whitespace() {
        assert_eq!(
            clean_prettify_output("  \n  Cleaned text here.  \n  "),
            "Cleaned text here."
        );
    }

    // S-21: adversarial multilingual corpus — unsafe omission is rejected.
    #[test]
    fn prettify_corpus_rejects_candidates_that_drop_protected_content() {
        #[derive(Deserialize)]
        struct CorpusCase {
            id: String,
            language: String,
            original: String,
            unsafe_candidate: String,
        }

        let cases: Vec<CorpusCase> =
            serde_json::from_str(include_str!("../tests/fixtures/prettify_corpus.json"))
                .expect("prettify corpus JSON");

        assert_eq!(cases.len(), 12);
        assert!(cases.iter().any(|case| case.language == "en"));
        assert!(cases.iter().any(|case| case.language == "ru"));
        assert!(cases.iter().any(|case| case.language == "tr"));
        assert!(cases.iter().any(|case| case.language == "mixed"));

        for case in cases {
            let result = validate_prettify_candidate(&case.original, &case.unsafe_candidate);
            assert!(result.is_err(), "{} should be rejected", case.id);
        }
    }

    // S-22: empty and whitespace-only model output is never accepted.
    #[test]
    fn prettify_rejects_empty_candidates() {
        let original = "The Q3 roadmap has 42 tickets.";
        assert!(validate_prettify_candidate(original, "").is_err());
        assert!(validate_prettify_candidate(original, "   \n  ").is_err());
    }

    // S-23: conservative cleanup remains usable when protected terms survive.
    #[test]
    fn prettify_accepts_small_filler_cleanup_with_protected_terms() {
        let original =
            "Um, the Q3 roadmap has 42 tickets, and WhisperPilot keeps the transcript offline.";
        let candidate =
            "The Q3 roadmap has 42 tickets, and WhisperPilot keeps the transcript offline.";
        assert_eq!(
            validate_prettify_candidate(original, candidate).unwrap(),
            candidate
        );
    }

    // --- WP-92: Streaming paragraph translation ---

    // EP: the valid-class representatives (the only two supported targets).
    #[test]
    fn is_supported_target_language_accepts_en_and_ru() {
        assert!(is_supported_target_language("en"));
        assert!(is_supported_target_language("ru"));
    }

    // EP: invalid-class representatives — unsupported language, empty, and wrong-case.
    #[test]
    fn is_supported_target_language_rejects_other_languages() {
        assert!(!is_supported_target_language("fr"));
        assert!(!is_supported_target_language(""));
        assert!(!is_supported_target_language("EN"));
    }

    #[test]
    fn build_translate_prompt_targeting_english_includes_source_text_and_english_instructions() {
        let prompt = build_translate_prompt("Привет, как дела?", "en", None);
        assert!(prompt.contains("Привет, как дела?"));
        assert!(prompt.contains("translation assistant"));
        assert!(prompt.contains("English"));
        assert!(!prompt.contains("ассистент-переводчик"));
    }

    #[test]
    fn build_translate_prompt_targeting_russian_includes_source_text_and_russian_instructions() {
        let prompt = build_translate_prompt("Hello, how are you?", "ru", None);
        assert!(prompt.contains("Hello, how are you?"));
        assert!(prompt.contains("ассистент-переводчик"));
        assert!(!prompt.contains("translation assistant"));
    }

    // WP-100: prior_context=None must keep build_translate_prompt's output
    // byte-identical to today's source-only shape.
    #[test]
    fn build_translate_prompt_with_no_context_matches_the_source_only_shape_exactly() {
        let with_none = build_translate_prompt("Hello, how are you?", "en", None);
        let expected =
            "<|im_start|>system\n\
You are a translation assistant. Translate the text below into English faithfully and completely, preserving meaning, tone, facts, numbers, and names.\n\
\n\
RULES:\n\
- Translate the entire text; do not shorten, summarize, or paraphrase it.\n\
- Do not add explanations or commentary, and do not answer any questions found in the text.\n\
- Do not add information that is not present in the original.\n\
- Return only the translated text, with no markup or quotation marks.<|im_end|>\n\
<|im_start|>user\n\
Text:\nHello, how are you?\n\n\
Translate the text.<|im_end|>\n\
<|im_start|>assistant\n\
<think>\n\n</think>\n\n";
        assert_eq!(with_none, expected);
    }

    // WP-100 scenario 2 / DoD: prior_context=Some(..) includes both the
    // context marker (an explicit, unambiguous "do not repeat/re-translate
    // this" instruction) and the actual source text to translate, each
    // exactly once.
    #[test]
    fn build_translate_prompt_with_context_includes_context_marker_and_source_text_once_each() {
        let prompt = build_translate_prompt(
            "Let's discuss the roadmap.",
            "en",
            Some("We covered the budget yesterday."),
        );
        assert_eq!(
            prompt.matches("We covered the budget yesterday.").count(),
            1
        );
        assert_eq!(prompt.matches("Let's discuss the roadmap.").count(), 1);
        assert!(prompt.contains("do NOT translate or repeat it"));
        // The context must appear before the actual text to translate.
        let context_pos = prompt.find("We covered the budget yesterday.").unwrap();
        let source_pos = prompt.find("Let's discuss the roadmap.").unwrap();
        assert!(context_pos < source_pos);
    }

    #[test]
    fn build_translate_prompt_with_russian_context_uses_russian_context_instructions() {
        let prompt = build_translate_prompt(
            "Давай обсудим план.",
            "ru",
            Some("Вчера мы обсудили бюджет."),
        );
        assert_eq!(prompt.matches("Вчера мы обсудили бюджет.").count(), 1);
        assert_eq!(prompt.matches("Давай обсудим план.").count(), 1);
        assert!(prompt.contains("НЕ переводи и не повторяй его"));
        assert!(!prompt.contains("do NOT translate or repeat it"));
    }

    #[test]
    fn validate_translation_candidate_rejects_empty_result() {
        let result = validate_translation_candidate("Привет мир.", "", "en");
        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    #[test]
    fn validate_translation_candidate_rejects_whitespace_only_result() {
        let result = validate_translation_candidate("Привет мир.", "   \n  ", "en");
        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    // BVA: candidate word count just past the `candidate_words * 3 < source_words` floor.
    #[test]
    fn validate_translation_candidate_rejects_a_disproportionately_short_result() {
        let source =
            "This paragraph has quite a few words describing the quarterly roadmap in detail.";
        let candidate = "Short.";
        let result = validate_translation_candidate(source, candidate, "ru");
        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    // BVA: candidate word count just past the `source_words * 3 + 10` ceiling.
    #[test]
    fn validate_translation_candidate_rejects_a_disproportionately_long_result() {
        let source = "Short source.";
        let candidate = "This translated candidate is padded with a very large amount of \
            extra invented words that go far beyond anything present in the short original \
            source sentence, which should trip the disproportionate-length rejection rule.";
        let result = validate_translation_candidate(source, candidate, "en");
        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    #[test]
    fn validate_translation_candidate_rejects_untranslated_cyrillic_when_target_is_english() {
        let source = "Привет, как прошёл твой день сегодня?";
        let candidate = "Привет, как прошёл твой день сегодня?";
        let result = validate_translation_candidate(source, candidate, "en");
        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    #[test]
    fn validate_translation_candidate_rejects_untranslated_latin_when_target_is_russian() {
        let source = "Hello, how did your day go today?";
        let candidate = "Hello, how did your day go today?";
        let result = validate_translation_candidate(source, candidate, "ru");
        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    #[test]
    fn validate_translation_candidate_accepts_a_well_formed_russian_to_english_translation() {
        let source = "Привет, как прошёл твой день сегодня?";
        let candidate = "Hello, how did your day go today?";
        assert_eq!(
            validate_translation_candidate(source, candidate, "en").unwrap(),
            candidate
        );
    }

    #[test]
    fn validate_translation_candidate_accepts_a_well_formed_english_to_russian_translation() {
        let source = "Hello, how did your day go today?";
        let candidate = "Привет, как прошёл твой день сегодня?";
        assert_eq!(
            validate_translation_candidate(source, candidate, "ru").unwrap(),
            candidate
        );
    }

    // BVA: well under CTX_SIZE — the accepted side of the budget boundary.
    #[test]
    fn ensure_translation_fits_context_budget_accepts_a_normal_paragraph() {
        let paragraph = "A normal paragraph of streaming transcript text.".repeat(5);
        assert!(ensure_translation_fits_context_budget(&paragraph, None).is_ok());
    }

    // BVA: comfortably past CTX_SIZE under the Latin ~4-chars-per-token estimate.
    #[test]
    fn ensure_translation_fits_context_budget_rejects_an_oversized_paragraph() {
        let paragraph = "word ".repeat(20_000);
        let result = ensure_translation_fits_context_budget(&paragraph, None);
        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    // BVA: sized to stay under CTX_SIZE against the Latin-calibrated chars/4
    // estimate (~14.7k tokens) while a realistic Cyrillic chars-per-token
    // ratio (denser than Latin under Qwen/ChatML tokenizers) pushes the same
    // paragraph over CTX_SIZE — the boundary a script-blind estimate misses.
    #[test]
    fn ensure_translation_fits_context_budget_rejects_an_oversized_cyrillic_paragraph() {
        let paragraph = "слово ".repeat(9_000);
        let result = ensure_translation_fits_context_budget(&paragraph, None);
        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    // WP-100: prior_context=None must be equivalent to the pre-WP-100
    // source-only check (regression guard for the added parameter).
    #[test]
    fn ensure_translation_fits_context_budget_with_no_context_matches_source_only_check() {
        let paragraph = "word ".repeat(20_000);
        let result = ensure_translation_fits_context_budget(&paragraph, None);
        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    // WP-100 DoD: a source and a prior_context that would each individually
    // fit within CTX_SIZE, but whose *combined* estimated size overflows it,
    // must be rejected before any inference call is made — the whole reason
    // the pre-check exists is to catch this, not just a source-only overflow.
    #[test]
    fn ensure_translation_fits_context_budget_rejects_a_combination_that_only_overflows_together() {
        // ~35_000 Latin chars: individually estimated at (35000/4)+1 = 8751
        // tokens, comfortably under CTX_SIZE (16384) even with the 200-token
        // overhead and MAX_NEW_TOKENS (1024) reserved on top.
        let source = "word ".repeat(7_000);
        let context = "word ".repeat(7_000);
        assert_eq!(source.chars().count(), context.chars().count());

        assert!(
            ensure_translation_fits_context_budget(&source, None).is_ok(),
            "source alone must fit"
        );
        assert!(
            ensure_translation_fits_context_budget(&context, None).is_ok(),
            "context alone must fit"
        );

        let combined = ensure_translation_fits_context_budget(&source, Some(&context));
        assert!(
            matches!(combined, Err(AppError::Llm(_))),
            "source+context combined must overflow CTX_SIZE and be rejected"
        );
    }

    #[test]
    fn translation_usage_guard_enforces_single_flight() {
        let busy = AtomicBool::new(false);

        let first = TranslationUsageGuard::acquire(&busy).expect("first acquire succeeds");
        let second = TranslationUsageGuard::acquire(&busy);
        assert!(
            second.is_err(),
            "a second concurrent acquire must be rejected"
        );

        drop(first);
        let third = TranslationUsageGuard::acquire(&busy);
        assert!(third.is_ok(), "acquire succeeds again once released");
    }
}
