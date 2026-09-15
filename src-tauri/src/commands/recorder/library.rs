//! Recorder library and persistence IPC commands.

use super::export;
use super::{ensure_recorder_session_is_not_live, RecorderSessionDto, RecorderSessionSummaryDto};
use crate::asr::{self, AsrLanguage, AsrMode};
use crate::commands::recorder_dto::open_dto;
use crate::error::{AppError, Result};
use crate::microphone_permission::{self, MicrophonePermissionStatus};
use crate::recorder_audio::export_caf_to_wav;
use crate::recorder_store::{
    NewRecorderSession, RecorderSegment, RecorderSessionId, RecorderStore,
};
use crate::state::{app_data_dir, now_ms};
use tauri::{Emitter, Manager};

#[tauri::command]
pub(crate) fn get_microphone_permission_status() -> MicrophonePermissionStatus {
    microphone_permission::get_microphone_permission_status()
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) fn show_recorder_workspace(app: tauri::AppHandle) -> Result<()> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::Capture("WhisperPilot main window is unavailable".into()))?;
    main.show()
        .map_err(|error| AppError::Io(error.to_string()))?;
    main.unminimize()
        .map_err(|error| AppError::Io(error.to_string()))?;
    app.emit_to("main", "open_recorder_workspace", ())
        .map_err(|error| AppError::Io(error.to_string()))?;
    main.set_focus()
        .map_err(|error| AppError::Io(error.to_string()))
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) fn show_recorder_workspace(_app: tauri::AppHandle) -> Result<()> {
    Err(AppError::Capture(
        "Recorder workspace is only available on macOS".into(),
    ))
}

#[tauri::command]
pub(crate) async fn request_microphone_permission() -> MicrophonePermissionStatus {
    microphone_permission::request_microphone_permission().await
}

#[tauri::command]
pub(crate) fn list_recorder_sessions(
    app: tauri::AppHandle,
) -> Result<Vec<RecorderSessionSummaryDto>> {
    let app_support_dir = app_data_dir(&app)?;
    let store = RecorderStore::open_runtime(&app_support_dir)?;
    Ok(store
        .list_sessions()?
        .into_iter()
        .map(RecorderSessionSummaryDto::from)
        .collect())
}

#[tauri::command]
pub(crate) fn open_recorder_session(
    app: tauri::AppHandle,
    id: RecorderSessionId,
) -> Result<RecorderSessionDto> {
    open_dto(&app_data_dir(&app)?, id)
}

#[tauri::command]
pub(crate) fn create_recorder_draft(app: tauri::AppHandle) -> Result<RecorderSessionDto> {
    let app_support_dir = app_data_dir(&app)?;
    let settings = crate::settings::get_settings(&app_support_dir);
    let language = AsrLanguage::Auto;
    let asr_spec = asr::resolve_selection(
        settings
            .active_model_transcription
            .as_deref()
            .unwrap_or(asr::DEFAULT_ASR_MODEL_ID),
        AsrMode::Recorder,
        language,
    )?;
    let now = now_ms()?;
    let session =
        RecorderStore::open_runtime(&app_support_dir)?.create_draft(NewRecorderSession {
            title: "Untitled recording".into(),
            created_at_ms: now,
            sample_rate: 48_000,
            asr_model_id: asr_spec.model_id.to_string(),
            asr_engine: asr_spec.engine.as_str().to_string(),
            asr_language: language.code().to_string(),
        })?;
    open_dto(&app_support_dir, session.id)
}

#[tauri::command]
pub(crate) fn rename_recorder_session(
    app: tauri::AppHandle,
    id: RecorderSessionId,
    title: String,
) -> Result<RecorderSessionDto> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::Store("Recorder title must not be empty".into()));
    }
    let app_support_dir = app_data_dir(&app)?;
    RecorderStore::open_runtime(&app_support_dir)?.rename_session(id, title)?;
    open_dto(&app_support_dir, id)
}

#[tauri::command]
pub(crate) fn delete_recorder_session(app: tauri::AppHandle, id: RecorderSessionId) -> Result<()> {
    ensure_recorder_session_is_not_live(&app, id)?;
    RecorderStore::open_runtime(&app_data_dir(&app)?)?.delete_session(id)
}

#[tauri::command]
pub(crate) fn clear_recorder_recording(
    app: tauri::AppHandle,
    id: RecorderSessionId,
) -> Result<RecorderSessionDto> {
    ensure_recorder_session_is_not_live(&app, id)?;
    let app_support_dir = app_data_dir(&app)?;
    RecorderStore::open_runtime(&app_support_dir)?.clear_recording(id)?;
    open_dto(&app_support_dir, id)
}

#[tauri::command]
pub(crate) fn update_recorder_segment(
    app: tauri::AppHandle,
    session_id: RecorderSessionId,
    segment_id: i64,
    text: String,
) -> Result<RecorderSegment> {
    let store = RecorderStore::open_runtime(&app_data_dir(&app)?)?;
    store.update_segment_text(session_id, segment_id, &text)?;
    store
        .get_segment(session_id, segment_id)?
        .ok_or_else(|| AppError::Store(format!("Recorder segment {segment_id} was not found")))
}

#[tauri::command]
pub(crate) fn recover_recorder_session(
    app: tauri::AppHandle,
    id: RecorderSessionId,
) -> Result<RecorderSessionDto> {
    ensure_recorder_session_is_not_live(&app, id)?;
    let app_support_dir = app_data_dir(&app)?;
    let store = RecorderStore::open_runtime(&app_support_dir)?;
    store.recover_session(id)?;
    open_dto(&app_support_dir, id)
}

#[tauri::command]
pub(crate) async fn export_recorder_wav(
    app: tauri::AppHandle,
    id: RecorderSessionId,
) -> Result<Option<String>> {
    let app_support_dir = app_data_dir(&app)?;
    let session = RecorderStore::open_runtime(&app_support_dir)?
        .get_session(id)?
        .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))?;
    let Some(path) = rfd::AsyncFileDialog::new()
        .add_filter("Wave audio", &["wav"])
        .set_file_name(format!("{}.wav", export::safe_file_stem(&session.title)))
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let target = path.path().to_path_buf();
    export_caf_to_wav(&session.audio_path, &target)?;
    Ok(Some(target.to_string_lossy().into_owned()))
}
