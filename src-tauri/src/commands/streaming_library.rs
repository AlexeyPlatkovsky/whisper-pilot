//! Persisted Meeting-library IPC commands.

use crate::error::Result;
use crate::state::{app_data_dir, now_ms};
use crate::streaming;
use crate::streaming_store;

/// List persisted Meeting sessions newest-touched first, for the library.
#[tauri::command]
pub(crate) fn list_streaming_sessions(
    app: tauri::AppHandle,
) -> Result<Vec<streaming::StreamingSessionSummaryDto>> {
    streaming::list_streaming_sessions(&app_data_dir(&app)?)
}

/// Open a complete persisted Meeting session (all decoded windows).
#[tauri::command]
pub(crate) fn open_streaming_session(
    app: tauri::AppHandle,
    id: i64,
) -> Result<streaming::StreamingSessionDto> {
    streaming::open_streaming_session(&app_data_dir(&app)?, id)
}

#[tauri::command]
pub(crate) fn rename_streaming_session(
    app: tauri::AppHandle,
    id: i64,
    title: String,
) -> Result<streaming::StreamingSessionDto> {
    streaming::rename_streaming_session(&app_data_dir(&app)?, id, title)
}

#[tauri::command]
pub(crate) fn delete_streaming_session(app: tauri::AppHandle, id: i64) -> Result<()> {
    streaming::delete_streaming_session(&app_data_dir(&app)?, id)
}

#[tauri::command]
pub(crate) fn clear_streaming_session(
    app: tauri::AppHandle,
    id: i64,
) -> Result<streaming::StreamingSessionDto> {
    streaming::clear_streaming_session(&app_data_dir(&app)?, id)
}

/// All persisted window translations for one session and target language.
#[tauri::command]
pub(crate) fn list_streaming_translations(
    app: tauri::AppHandle,
    session_id: streaming_store::StreamingSessionId,
    target_language: String,
) -> Result<Vec<streaming::StreamingTranslationDto>> {
    streaming::list_streaming_translations(&app_data_dir(&app)?, session_id, &target_language)
}

/// Create a stopped Meeting session record. Capture begins only after
/// `start_streaming_session` is invoked for the returned item.
#[tauri::command]
pub(crate) fn create_streaming_session(
    app: tauri::AppHandle,
) -> Result<streaming::StreamingSessionSummaryDto> {
    let app_support_dir = app_data_dir(&app)?;
    let id = streaming::create_streaming_session(&app_support_dir, now_ms()?)?;
    let session = streaming::open_streaming_session(&app_support_dir, id)?;
    Ok(streaming::StreamingSessionSummaryDto {
        id: session.id,
        title: session.title,
        created_at_ms: session.created_at_ms,
        updated_at_ms: session.updated_at_ms,
        duration_ms: 0,
        status: session.status,
        translation_enabled: session.translation_enabled,
        translation_target_language: session.translation_target_language,
    })
}

#[tauri::command]
pub(crate) fn set_streaming_translation_enabled(
    app: tauri::AppHandle,
    session_id: streaming_store::StreamingSessionId,
    enabled: bool,
) -> Result<()> {
    streaming::set_streaming_translation_enabled(&app_data_dir(&app)?, session_id, enabled)
}

#[tauri::command]
pub(crate) fn set_streaming_translation_target_language(
    app: tauri::AppHandle,
    session_id: streaming_store::StreamingSessionId,
    target_language: String,
) -> Result<()> {
    streaming::set_streaming_translation_target_language(
        &app_data_dir(&app)?,
        session_id,
        &target_language,
    )
}
