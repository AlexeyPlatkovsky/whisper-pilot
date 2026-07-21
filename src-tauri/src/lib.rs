//! MFUPilot core: offline file transcription (Russian) with a summary to come.

pub mod audio;
pub mod error;
pub mod transcribe;

use error::{AppError, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;
use transcribe::Segment;
use whisper_rs::WhisperContext;

/// The whisper model is loaded lazily on first use and cached for the session —
/// loading ~800 MB should not block app launch or fail startup when the model
/// is missing.
#[derive(Default)]
struct AppState {
    model: Mutex<Option<Arc<WhisperContext>>>,
}

impl AppState {
    async fn model(&self) -> Result<Arc<WhisperContext>> {
        let mut guard = self.model.lock().await;
        if let Some(ctx) = guard.as_ref() {
            return Ok(Arc::clone(ctx));
        }
        // Loading is blocking and CPU-heavy; keep it off the async reactor.
        let ctx = tokio::task::spawn_blocking(transcribe::load_model)
            .await
            .map_err(|e| AppError::ModelLoad(e.to_string()))??;
        let ctx = Arc::new(ctx);
        *guard = Some(Arc::clone(&ctx));
        Ok(ctx)
    }
}

/// Result of transcribing one file.
#[derive(serde::Serialize)]
struct TranscriptResult {
    file_name: String,
    segments: Vec<Segment>,
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

/// Transcribe the file at `path` into Russian (default) timestamped segments.
#[tauri::command]
async fn transcribe_file(
    path: String,
    language: Option<String>,
    state: State<'_, AppState>,
) -> Result<TranscriptResult> {
    let input = PathBuf::from(&path);
    let file_name = input
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let language = language.unwrap_or_else(|| "ru".to_string());

    let ctx = state.model().await?;
    let segments = tokio::task::spawn_blocking(move || {
        transcribe::transcribe_file(&ctx, &input, &language)
    })
    .await
    .map_err(|e| AppError::Transcribe(e.to_string()))??;

    Ok(TranscriptResult {
        file_name,
        segments,
    })
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            open_file_dialog,
            transcribe_file,
            save_text_dialog
        ])
        .run(tauri::generate_context!())
        .expect("error while running MFUPilot");
}
