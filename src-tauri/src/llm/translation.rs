use super::policy::{clean_prettify_output, normalize_protected_token, protected_tokens};
use super::runtime::{CTX_SIZE, MAX_NEW_TOKENS};
use super::*;

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
pub(crate) fn build_translate_prompt(
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
- Не транслитерируй имена.\n\
- Копируй дословно каждый идентификатор, фрагмент кода, токен с цифрами, аббревиатуру и код валюты (например, USD); сохраняй написание и регистр.\n\
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
- Do not transliterate names.\n\
- Copy every identifier, code fragment, token containing digits, acronym, and currency code (for example, USD) exactly; preserve spelling and case.\n\
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

pub(crate) fn ensure_prompt_fits_context_budget(prompt: &str) -> Result<()> {
    run_with_token_preflight(
        estimated_token_count(prompt),
        MAX_NEW_TOKENS as usize,
        CTX_SIZE as usize,
        || Ok(()),
    )
}

/// `prior_context`'s estimated length is added on top of `source_text`'s
/// (WP-100) — reusing `estimated_token_count`'s own script-aware estimate
/// for each rather than adding a second detection path — so a combined
/// prompt that would overflow `CTX_SIZE` is rejected here, before an
/// inference call is spent on it, exactly as a source-only overflow already
/// was.
pub(crate) fn ensure_translation_fits_context_budget(
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

pub(crate) fn validate_translation_candidate(
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

    for token in protected_tokens(source) {
        let normalized = normalize_protected_token(token);
        let contains_digit = normalized.chars().any(|c| c.is_ascii_digit());
        let alphabetic_parts = normalized
            .split(|c: char| !c.is_ascii_alphabetic())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let protected_parts = alphabetic_parts
            .iter()
            .copied()
            .filter(|part| part.chars().filter(|c| c.is_ascii_uppercase()).count() >= 2)
            .collect::<Vec<_>>();
        let whole_ascii_code_like = !alphabetic_parts.is_empty()
            && protected_parts.len() == alphabetic_parts.len()
            && normalized
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '%'));
        if !contains_digit && !normalized.contains('_') && !whole_ascii_code_like {
            for protected_part in protected_parts {
                let retained = candidate
                    .split_whitespace()
                    .map(normalize_protected_token)
                    .flat_map(|candidate_token| {
                        candidate_token.split(|c: char| !c.is_ascii_alphabetic())
                    })
                    .any(|candidate_part| candidate_part == protected_part);
                if !retained {
                    return Err(AppError::Llm(format!(
                        "translation dropped protected term '{protected_part}'; review was rejected"
                    )));
                }
            }
            continue;
        }
        let retained = candidate
            .split_whitespace()
            .map(normalize_protected_token)
            .any(|candidate_token| candidate_token == normalized);
        if !retained {
            return Err(AppError::Llm(format!(
                "translation dropped protected term '{normalized}'; review was rejected"
            )));
        }
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
    runtime: &LlmRuntime,
    model_path: &Path,
    source_text: &str,
    target_language: &str,
    prior_context: Option<&str>,
) -> Result<String> {
    translate_paragraph_with_inference(source_text, target_language, prior_context, |prompt| {
        runtime.infer(model_path, LlmJobKind::Translation, prompt)
    })
}

pub fn translate_paragraph_with_model_resolver(
    runtime: &LlmRuntime,
    source_text: &str,
    target_language: &str,
    prior_context: Option<&str>,
    mut resolve_model: impl FnMut() -> Result<std::path::PathBuf>,
) -> Result<String> {
    translate_paragraph_with_inference(source_text, target_language, prior_context, |prompt| {
        runtime.infer_with_model_resolver(LlmJobKind::Translation, prompt, &mut resolve_model)
    })
}

/// Lower-priority, non-persisted translation of an unstable Streaming
/// hypothesis. A committed translation can enter the scheduler ahead of a
/// queued preview, while the shared model weights remain cached.
pub fn preview_translation_with_model_resolver(
    runtime: &LlmRuntime,
    source_text: &str,
    target_language: &str,
    prior_context: Option<&str>,
    mut resolve_model: impl FnMut() -> Result<std::path::PathBuf>,
) -> Result<String> {
    translate_paragraph_with_inference(source_text, target_language, prior_context, |prompt| {
        runtime.infer_with_model_resolver(
            LlmJobKind::TranslationPreview,
            prompt,
            &mut resolve_model,
        )
    })
}

pub(crate) fn translate_paragraph_with_inference(
    source_text: &str,
    target_language: &str,
    prior_context: Option<&str>,
    mut inference: impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    ensure_translation_fits_context_budget(source_text, prior_context)?;
    let prompt = build_translate_prompt(source_text, target_language, prior_context);
    let raw_output = inference(&prompt)?;
    let cleaned = clean_prettify_output(&raw_output);
    match validate_translation_candidate(source_text, &cleaned, target_language) {
        Ok(candidate) => Ok(candidate),
        Err(first_error) if prior_context.is_some() => {
            // Context improves continuity, but a small local model can
            // occasionally echo it or under-translate. Retry once without
            // context before surfacing a stable per-window failure.
            let fallback_prompt = build_translate_prompt(source_text, target_language, None);
            let fallback_raw = inference(&fallback_prompt)?;
            let fallback = clean_prettify_output(&fallback_raw);
            validate_translation_candidate(source_text, &fallback, target_language).map_err(
                |fallback_error| {
                    AppError::Llm(format!(
                        "translation failed with context ({first_error}) and without context ({fallback_error})"
                    ))
                },
            )
        }
        Err(error) => Err(error),
    }
}
