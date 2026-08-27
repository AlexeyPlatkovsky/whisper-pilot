//! LLM MFU and prettify IPC commands: structured meeting MFU (Meeting and
//! Streaming) and Streaming transcript prettify accept/revert.

use crate::error::{AppError, Result};
use crate::llm;
use crate::meetings::MeetingDto;
use crate::models;
use crate::settings;
use crate::state::app_data_dir;
use crate::store;
use crate::streaming;
use crate::streaming_store;
use std::path::PathBuf;

/// Shared by both Craft/MFU commands (Meeting and Streaming). Errors name the
/// exact missing prerequisite rather than a generic failure.
fn resolve_llm_model_path(app_support_dir: &std::path::Path) -> Result<PathBuf> {
    let settings = settings::get_settings(app_support_dir);
    let llm_model_id = settings
        .active_model_llm
        .as_deref()
        .ok_or_else(|| AppError::Llm("no LLM model selected in Settings".into()))?;

    let model_path = models::primary_asset_path(app_support_dir, llm_model_id)
        .ok_or_else(|| AppError::Llm(format!("LLM model {llm_model_id} is not downloaded")))?;
    if !model_path.exists() {
        return Err(AppError::Llm(format!(
            "LLM model file not found at {}",
            model_path.display()
        )));
    }
    Ok(model_path)
}

/// Generate structured meeting MFU (summary, decisions, action items, open
/// questions, participants) from the current transcript using the active LLM
/// model. Requires an LLM model to be downloaded and selected in Settings.
#[tauri::command]
pub(crate) async fn generate_mfu(app: tauri::AppHandle, id: i64) -> Result<MeetingDto> {
    let app_support_dir = app_data_dir(&app)?;
    let model_path = resolve_llm_model_path(&app_support_dir)?;

    let store = store::Store::open(&app_support_dir)?;
    let _meeting = store
        .get_meeting(id)?
        .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;

    let segments = store.list_segments(id)?;
    if segments.is_empty() {
        return Err(AppError::Llm(
            "meeting has no transcript to summarize".into(),
        ));
    }

    let transcript: String = segments
        .iter()
        .map(|seg| {
            if let Some(sid) = seg.speaker_id {
                format!("Speaker {}: {}", sid, seg.text)
            } else {
                seg.text.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let app_support_dir_clone = app_support_dir.clone();
    let model_path_clone = model_path.clone();
    let transcript_clone = transcript.clone();
    let generated = tokio::task::spawn_blocking(move || {
        llm::generate_mfu(&model_path_clone, &transcript_clone)
    })
    .await
    .map_err(|e| AppError::Llm(e.to_string()))??;

    store.upsert_mfu(&store::MeetingMfu {
        meeting_id: id,
        summary: generated.summary,
        decisions: generated.decisions,
        action_items: generated.action_items,
        open_questions: generated.open_questions,
        participants: generated.participants,
    })?;

    crate::meetings::open_meeting(&app_support_dir_clone, id)
}

/// Same local model/JSON contract as `generate_mfu`, for a Streaming
/// session — `streaming::build_streaming_transcript` owns its guards.
#[tauri::command]
pub(crate) async fn generate_streaming_mfu(
    app: tauri::AppHandle,
    id: i64,
) -> Result<streaming::StreamingSessionDto> {
    let app_support_dir = app_data_dir(&app)?;
    let model_path = resolve_llm_model_path(&app_support_dir)?;
    let transcript = streaming::build_streaming_transcript(&app_support_dir, id)?;

    let model_path_clone = model_path.clone();
    let transcript_clone = transcript.clone();
    let generated = tokio::task::spawn_blocking(move || {
        llm::generate_mfu(&model_path_clone, &transcript_clone)
    })
    .await
    .map_err(|e| AppError::Llm(e.to_string()))??;

    let store = streaming_store::StreamingStore::open(&app_support_dir)?;
    store.upsert_mfu(&streaming_store::StreamingMfu {
        session_id: id,
        summary: generated.summary,
        decisions: generated.decisions,
        action_items: generated.action_items,
        open_questions: generated.open_questions,
        participants: generated.participants,
    })?;

    streaming::open_streaming_session(&app_support_dir, id)
}

/// Returns the cleaned transcript without persisting it — the frontend
/// shows it as a diff for review; only `accept_streaming_prettify` writes it.
#[tauri::command]
pub(crate) async fn generate_streaming_prettify(app: tauri::AppHandle, id: i64) -> Result<String> {
    let app_support_dir = app_data_dir(&app)?;
    let model_path = resolve_llm_model_path(&app_support_dir)?;
    let transcript = streaming::build_streaming_transcript(&app_support_dir, id)?;

    tokio::task::spawn_blocking(move || llm::prettify_transcript(&model_path, &transcript))
        .await
        .map_err(|e| AppError::Llm(e.to_string()))?
}

#[tauri::command]
pub(crate) async fn accept_streaming_prettify(
    app: tauri::AppHandle,
    id: i64,
    text: String,
) -> Result<streaming::StreamingSessionDto> {
    let app_support_dir = app_data_dir(&app)?;
    let store = streaming_store::StreamingStore::open(&app_support_dir)?;
    store.upsert_prettified(id, &text)?;
    streaming::open_streaming_session(&app_support_dir, id)
}

/// Removes an accepted prettification so the raw per-window transcript is shown again.
#[tauri::command]
pub(crate) async fn revert_streaming_prettify(
    app: tauri::AppHandle,
    id: i64,
) -> Result<streaming::StreamingSessionDto> {
    let app_support_dir = app_data_dir(&app)?;
    let store = streaming_store::StreamingStore::open(&app_support_dir)?;
    store.delete_prettified(id)?;
    streaming::open_streaming_session(&app_support_dir, id)
}

/// Translates one Streaming paragraph into `target_language` ("en" or "ru")
/// using the active local LLM and persists the result. Single-flight: a
/// second concurrent translation request is rejected with a distinct,
/// UI-retryable `AppError::TranslationBusy` rather than queuing or blocking
/// the streaming decode loop (WP-92). `context` (WP-100) is the immediately
/// preceding paragraph's own translation, if any — threaded through
/// unchanged to `llm::translate_paragraph` as ephemeral prompt context.
#[tauri::command]
pub(crate) async fn translate_streaming_paragraph(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    session_id: streaming_store::StreamingSessionId,
    paragraph_key: i64,
    target_language: String,
    text: String,
    context: Option<String>,
) -> Result<String> {
    let app_support_dir = app_data_dir(&app)?;
    streaming::ensure_translation_request_is_valid(
        &app_support_dir,
        session_id,
        &target_language,
        &text,
    )?;
    let model_path = resolve_llm_model_path(&app_support_dir)?;

    let _guard = llm::TranslationUsageGuard::acquire(&state.translation_busy)
        .map_err(|()| AppError::TranslationBusy)?;

    let now = crate::state::now_ms()?;
    let app_support_dir_clone = app_support_dir.clone();
    let target_language_clone = target_language.clone();
    let text_clone = text.clone();
    let model_path_clone = model_path.clone();
    tokio::task::spawn_blocking(move || {
        streaming::translate_and_store(
            &app_support_dir_clone,
            session_id,
            paragraph_key,
            &target_language_clone,
            &text_clone,
            context.as_deref(),
            now,
            |source, lang, ctx| llm::translate_paragraph(&model_path_clone, source, lang, ctx),
        )
    })
    .await
    .map_err(|e| AppError::Llm(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- WP-92: model-resolution error path for translate_streaming_paragraph ---

    #[test]
    fn resolve_llm_model_path_errors_when_no_model_is_selected() {
        let temp = tempfile::tempdir().expect("temp dir");

        let result = resolve_llm_model_path(temp.path());

        assert!(matches!(result, Err(AppError::Llm(_))));
    }
}
