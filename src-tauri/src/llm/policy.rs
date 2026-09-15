use super::*;

#[derive(Debug, Deserialize)]
struct MfuJson {
    summary: JsonValue,
    decisions: JsonValue,
    #[serde(rename = "action_items")]
    action_items: JsonValue,
    #[serde(rename = "open_questions")]
    open_questions: JsonValue,
    participants: JsonValue,
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

pub(crate) const ASSISTANT_PREFILL: &str = "{\"summary\":\"";

pub(crate) fn build_prompt(transcript: &str) -> String {
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

pub fn strip_internal_reasoning(raw: &str) -> String {
    let mut cleaned = raw.trim();
    if let Some(after_open) = cleaned.strip_prefix("<think>") {
        cleaned = if let Some(idx) = after_open.find("</think>") {
            after_open[idx + 8..].trim_start()
        } else if let Some(idx) = after_open.find("\n\n") {
            after_open[idx + 2..].trim_start()
        } else {
            ""
        };
    }
    if let Some(channel_end) = cleaned.find("<channel|>") {
        cleaned = cleaned[channel_end + "<channel|>".len()..].trim_start();
    }
    if let Some(fence_start) = cleaned.find("```") {
        let after_open = &cleaned[fence_start + 3..];
        let after_language = after_open
            .strip_prefix("json")
            .or_else(|| after_open.strip_prefix("text"))
            .unwrap_or(after_open)
            .trim_start_matches([' ', '\n', '\r']);
        if let Some(fence_end) = after_language.find("```") {
            cleaned = &after_language[..fence_end];
        }
    } else if let Some(next_channel) = cleaned.find("<|channel>") {
        cleaned = &cleaned[..next_channel];
    }
    cleaned = cleaned.strip_prefix("```json").unwrap_or(cleaned);
    cleaned = cleaned.strip_prefix("```text").unwrap_or(cleaned);
    cleaned = cleaned.strip_prefix("```").unwrap_or(cleaned);
    cleaned = cleaned.strip_prefix('\n').unwrap_or(cleaned);
    cleaned = cleaned.strip_suffix("```").unwrap_or(cleaned);
    loop {
        let before = cleaned;
        for token in ["<|im_end|>", "<|endoftext|>", "<end_of_turn>", "<eos>"] {
            cleaned = cleaned.strip_suffix(token).unwrap_or(cleaned).trim_end();
        }
        if cleaned == before {
            break;
        }
    }
    cleaned.trim().to_string()
}

pub(crate) fn parse_notes_json(raw: &str) -> Result<GeneratedMfu> {
    let cleaned_owned = strip_internal_reasoning(raw);
    let cleaned = cleaned_owned.as_str();

    let json_str = if let Some(object) = first_complete_json_object(cleaned) {
        object.to_string()
    } else {
        format!("{ASSISTANT_PREFILL}{cleaned}")
    };

    let json_str = json_str.strip_suffix("```").unwrap_or(&json_str);
    let json_str = json_str.trim();

    if let Ok(parsed) = serde_json::from_str::<MfuJson>(json_str) {
        return Ok(GeneratedMfu {
            summary: mfu_value_to_text(parsed.summary),
            decisions: mfu_value_to_text(parsed.decisions),
            action_items: mfu_value_to_text(parsed.action_items),
            open_questions: mfu_value_to_text(parsed.open_questions),
            participants: mfu_value_to_text(parsed.participants),
        });
    }

    Err(AppError::Llm(
        "model did not return valid structured MFU JSON".to_string(),
    ))
}

pub(crate) fn first_complete_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0_u32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&text[start..start + offset + ch.len_utf8()]);
                }
            }
            _ => {}
        }
    }
    None
}

fn mfu_value_to_text(value: JsonValue) -> String {
    match value {
        JsonValue::Null => String::new(),
        JsonValue::String(text) => text,
        JsonValue::Array(values) => values
            .into_iter()
            .map(mfu_value_to_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        JsonValue::Object(fields) => fields
            .into_iter()
            .map(|(key, value)| format!("{key}: {}", mfu_value_to_text(value)))
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

pub fn generate_mfu(
    runtime: &LlmRuntime,
    model_path: &Path,
    transcript: &str,
) -> Result<GeneratedMfu> {
    generate_mfu_with_inference(transcript, |prompt| {
        runtime.infer(model_path, LlmJobKind::Mfu, prompt)
    })
}

pub fn generate_mfu_with_model_resolver(
    runtime: &LlmRuntime,
    transcript: &str,
    resolve_model: impl FnOnce() -> Result<std::path::PathBuf>,
) -> Result<GeneratedMfu> {
    generate_mfu_with_inference(transcript, |prompt| {
        runtime.infer_with_model_resolver(LlmJobKind::Mfu, prompt, resolve_model)
    })
}

pub fn generate_mfu_with_inference(
    transcript: &str,
    inference: impl FnOnce(&str) -> Result<String>,
) -> Result<GeneratedMfu> {
    let prompt = build_prompt(transcript);
    ensure_prompt_fits_context_budget(&prompt)?;
    parse_notes_json(&inference(&prompt)?)
}

pub(crate) fn build_prettify_prompt(transcript: &str) -> String {
    let is_russian = transcript
        .chars()
        .any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));

    let (system, user) = if is_russian {
        (
            "Ты — ассистент для осторожной очистки расшифровок. Отредактируй текст, сохраняя его содержание:\n\
убери только очевидные слова-паразиты и соседние повторы, а грамматику исправляй минимально.\n\
Исправляй ошибку распознавания речи в одном слове или склеенной паре только когда исходный вариант явно\n\
невозможен по контексту, а правильный вариант однозначен и фонетически близок.\n\
\n\
ПРАВИЛА:\n\
- Не удаляй законченные предложения, факты, имена, числа, технические термины или фрагменты на другом языке.\n\
- Не нормализуй сленг, разговорную речь, жаргон, названия, бренды, аббревиатуры и авторские формулировки.\n\
- Не используй стилистическое предпочтение как основание для замены слова или перефразирования.\n\
- Если есть несколько правдоподобных исправлений, оставь исходный фрагмент без изменений.\n\
- Сохрани исходный смысл, порядок мыслей и все языковые переключения — не добавляй новую информацию.\n\
- Если не уверен, оставь исходный фрагмент без изменений.\n\
- Верни только очищенный текст, без пояснений и без разметки.",
            "Очисти расшифровку.",
        )
    } else {
        (
            "You are a conservative transcript cleanup assistant. Edit the transcript while preserving its content:\n\
remove only obvious filler words and adjacent repetitions, and make minimal grammar fixes.\n\
Correct a speech-recognition error in one word or an obviously merged pair only when the original wording is\n\
contextually impossible, and the intended wording is overwhelmingly clear and phonetically close.\n\
\n\
RULES:\n\
- Do not delete complete sentences, facts, names, numbers, technical terms, or passages in another language.\n\
- Do not normalize slang, casual speech, jargon, names, brands, acronyms, or the speaker's personal wording.\n\
- Do not use stylistic preference as a reason to replace a word or paraphrase a passage.\n\
- If there are multiple plausible corrections, copy the original fragment unchanged.\n\
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
pub(crate) fn clean_prettify_output(raw: &str) -> String {
    strip_internal_reasoning(raw)
}

pub fn prettify_transcript(
    runtime: &LlmRuntime,
    model_path: &Path,
    transcript: &str,
) -> Result<String> {
    prettify_transcript_with_inference(transcript, |prompt| {
        runtime.infer(model_path, LlmJobKind::Prettify, prompt)
    })
}

pub fn prettify_transcript_with_model_resolver(
    runtime: &LlmRuntime,
    transcript: &str,
    resolve_model: impl FnOnce() -> Result<std::path::PathBuf>,
) -> Result<String> {
    prettify_transcript_with_inference(transcript, |prompt| {
        runtime.infer_with_model_resolver(LlmJobKind::Prettify, prompt, resolve_model)
    })
}

fn prettify_transcript_with_inference(
    transcript: &str,
    inference: impl FnOnce(&str) -> Result<String>,
) -> Result<String> {
    let prompt = build_prettify_prompt(transcript);
    ensure_prompt_fits_context_budget(&prompt)?;
    let raw_output = inference(&prompt)?;
    let cleaned = clean_prettify_output(&raw_output);
    validate_prettify_candidate(transcript, &cleaned)
}

pub(crate) fn protected_tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace().filter(|token| {
        let token = normalize_protected_token(token);
        let has_digit = token.chars().any(|c| c.is_ascii_digit());
        let has_separator = token.contains('-') || token.contains('_');
        let uppercase_count = token.chars().filter(|c| c.is_uppercase()).count();
        !token.is_empty() && (has_digit || has_separator || uppercase_count >= 2)
    })
}

pub(crate) fn normalize_protected_token(token: &str) -> &str {
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

pub(crate) fn validate_prettify_candidate(original: &str, candidate: &str) -> Result<String> {
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
