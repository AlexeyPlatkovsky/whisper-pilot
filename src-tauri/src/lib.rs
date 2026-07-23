//! WhisperPilot core: offline file transcription (Russian) with a summary to come.

pub mod audio;
pub mod diarize;
pub mod error;
pub mod meetings;
pub mod models;
pub mod settings;
pub mod store;
pub mod transcribe;

use error::{AppError, Result};
use meetings::{MeetingDto, MeetingSummaryDto};
use models::TaskModel;
use settings::Settings;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager, State};
use tokio::sync::Mutex;
use transcribe::Segment;
use whisper_rs::WhisperContext;

fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Io(e.to_string()))
}

fn now_ms() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::Store(error.to_string()))
        .and_then(|duration| {
            i64::try_from(duration.as_millis())
                .map_err(|_| AppError::Store("current time exceeds i64 milliseconds".into()))
        })
}

/// The whisper model is loaded lazily on first use and cached for the session —
/// loading ~800 MB should not block app launch or fail startup when the model
/// is missing.
#[derive(Default)]
struct AppState {
    model: Mutex<Option<Arc<WhisperContext>>>,
}

impl AppState {
    async fn model(&self, app_support_dir: PathBuf) -> Result<Arc<WhisperContext>> {
        let mut guard = self.model.lock().await;
        if let Some(ctx) = guard.as_ref() {
            return Ok(Arc::clone(ctx));
        }
        // Loading is blocking and CPU-heavy; keep it off the async reactor.
        let ctx = tokio::task::spawn_blocking(move || transcribe::load_model(&app_support_dir))
            .await
            .map_err(|e| AppError::ModelLoad(e.to_string()))??;
        let ctx = Arc::new(ctx);
        *guard = Some(Arc::clone(&ctx));
        Ok(ctx)
    }
}

/// Open a native file picker for audio/video and return the chosen path.
#[tauri::command]
async fn open_file_dialog() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .add_filter(
                "Audio/Video",
                &[
                    "mp3", "wav", "m4a", "aac", "flac", "ogg", "opus", "mp4", "mov", "mkv", "webm",
                    "avi", "m4v",
                ],
            )
            .add_filter("All files", &["*"])
            .pick_file()
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .ok()
    .flatten()
}

/// Decode, transcribe, and diarize the file at `path`. Diarization runs
/// automatically after transcription; any failure there (models missing,
/// engine error, or the task panicking) degrades to plain speaker-less
/// segments rather than failing the transcription.
async fn run_transcription(
    app_support_dir: PathBuf,
    ctx: Arc<WhisperContext>,
    path: String,
    language: String,
) -> Result<Vec<Segment>> {
    let input = PathBuf::from(&path);

    // Decode once (off the reactor); both transcription and diarization run
    // over the same samples.
    let samples = tokio::task::spawn_blocking(move || audio::load_samples(&input))
        .await
        .map_err(|e| AppError::Transcribe(e.to_string()))??;
    let samples_for_diarize = samples.clone();
    let mut segments =
        tokio::task::spawn_blocking(move || transcribe::transcribe(&ctx, &samples, &language))
            .await
            .map_err(|e| AppError::Transcribe(e.to_string()))??;

    let diarize_outcome = tokio::task::spawn_blocking(move || {
        diarize::diarize_samples(&app_support_dir, samples_for_diarize, None)
    })
    .await;
    diarize::apply_diarization_outcome(&mut segments, diarize_outcome);

    Ok(segments)
}

/// Attach (or clear, when `path` is `None`) the source file of a meeting.
/// Selecting the file is separate from running the transcription.
#[tauri::command]
fn set_meeting_source(app: tauri::AppHandle, id: i64, path: Option<String>) -> Result<MeetingDto> {
    meetings::set_meeting_source(&app_data_dir(&app)?, id, path)
}

/// Transcribe the meeting's attached source file into Russian (default)
/// timestamped segments and persist the result against the meeting.
#[tauri::command]
async fn transcribe_meeting(
    app: tauri::AppHandle,
    id: i64,
    state: State<'_, AppState>,
) -> Result<MeetingDto> {
    let app_support_dir = app_data_dir(&app)?;
    let meeting = meetings::open_meeting(&app_support_dir, id)?;
    let language = meeting.language;
    let path = meeting.source_path.ok_or_else(|| {
        AppError::Transcribe("meeting has no source file to transcribe".to_string())
    })?;

    let ctx = state.model(app_support_dir.clone()).await?;
    let segments = run_transcription(app_support_dir.clone(), ctx, path, language).await?;

    let duration_ms = segments.last().map(|segment| segment.end_ms as i64);
    let dtos = segments
        .into_iter()
        .map(|segment| meetings::SegmentDto {
            start_ms: segment.start_ms as i64,
            end_ms: segment.end_ms as i64,
            text: segment.text,
            speaker_id: segment.speaker_id.map(i64::from),
        })
        .collect();

    meetings::save_transcript(&app_support_dir, id, dtos, duration_ms)
}

/// Save `content` to a user-chosen destination; return the path written.
#[tauri::command]
async fn save_text_dialog(content: String, default_name: Option<String>) -> Result<Option<String>> {
    let name = default_name.unwrap_or_else(|| "transcript.txt".to_string());
    let picked = tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_file_name(&name)
            .add_filter("Text", &["txt", "md"])
            .save_file()
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?;

    let Some(dest) = picked else {
        return Ok(None);
    };
    std::fs::write(&dest, content)?;
    Ok(Some(dest.to_string_lossy().to_string()))
}

/// Create and return an empty persisted meeting. Attaching a file and starting
/// transcription are separate, explicit actions.
#[tauri::command]
fn create_meeting(app: tauri::AppHandle) -> Result<MeetingDto> {
    meetings::create_empty_meeting(&app_data_dir(&app)?, now_ms()?)
}

/// List persisted meetings newest first for the library sidebar.
#[tauri::command]
fn list_meetings(app: tauri::AppHandle) -> Result<Vec<MeetingSummaryDto>> {
    meetings::list_meetings(&app_data_dir(&app)?)
}

/// Open a complete persisted meeting for the active workspace.
#[tauri::command]
fn open_meeting(app: tauri::AppHandle, id: i64) -> Result<MeetingDto> {
    meetings::open_meeting(&app_data_dir(&app)?, id)
}

#[tauri::command]
fn rename_meeting(app: tauri::AppHandle, id: i64, title: String) -> Result<MeetingDto> {
    meetings::rename_meeting(&app_data_dir(&app)?, id, title)
}

#[tauri::command]
fn delete_meeting(app: tauri::AppHandle, id: i64) -> Result<()> {
    meetings::delete_meeting(&app_data_dir(&app)?, id)
}

/// Read all settings (theme, ui_language, active model), applying beta
/// defaults for any key never set.
#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<Settings> {
    let dir = app_data_dir(&app)?;
    Ok(settings::get_settings(&dir))
}

/// Update one known setting (theme, ui_language, or active_model.transcription)
/// and persist it immediately; rejects an unknown key or an invalid value.
#[tauri::command]
fn set_setting(app: tauri::AppHandle, key: String, value: String) -> Result<Settings> {
    let dir = app_data_dir(&app)?;
    settings::set_setting(&dir, &key, &value)
}

/// List the AI models catalog (transcription, diarization) with each
/// entry's current downloaded state.
#[tauri::command]
fn list_task_models(app: tauri::AppHandle) -> Result<Vec<TaskModel>> {
    let dir = app_data_dir(&app)?;
    Ok(models::list_task_models(&dir))
}

/// Download the catalog entry `id`, verifying SHA-256 before marking it
/// ready. Emits `model_download_progress { id, fraction }` as bytes arrive.
#[tauri::command]
async fn download_model(app: tauri::AppHandle, id: String) -> Result<()> {
    let dir = app_data_dir(&app)?;
    let progress_app = app.clone();
    let progress_id = id.clone();
    models::download_model(&dir, &id, move |fraction| {
        let _ = progress_app.emit(
            "model_download_progress",
            serde_json::json!({ "id": progress_id, "fraction": fraction }),
        );
    })
    .await
}

/// Delete catalog entry `id`'s downloaded file(s), returning it to
/// not-downloaded.
#[tauri::command]
fn delete_model(app: tauri::AppHandle, id: String) -> Result<()> {
    let dir = app_data_dir(&app)?;
    models::delete_model(&dir, &id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            open_file_dialog,
            create_meeting,
            list_meetings,
            open_meeting,
            rename_meeting,
            delete_meeting,
            set_meeting_source,
            transcribe_meeting,
            save_text_dialog,
            get_settings,
            set_setting,
            list_task_models,
            download_model,
            delete_model
        ])
        .run(tauri::generate_context!())
        .expect("error while running WhisperPilot");
}
