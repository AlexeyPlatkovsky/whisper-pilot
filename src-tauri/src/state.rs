//! Tauri app state: the cached Whisper context, the mutual-exclusion flag
//! shared with Streaming and the running Streaming capture runtime.

use crate::error::{AppError, Result};
#[cfg(target_os = "macos")]
use crate::streaming_audio;
use crate::transcribe;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;
use tokio::sync::Mutex;
use whisper_rs::WhisperContext;

pub(crate) fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Io(e.to_string()))
}

pub(crate) fn now_ms() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::Store(error.to_string()))
        .and_then(|duration| {
            i64::try_from(duration.as_millis())
                .map_err(|_| AppError::Store("current time exceeds i64 milliseconds".into()))
        })
}

/// Streaming's whisper model is loaded lazily on first use and cached for the
/// session — loading ~800 MB should not block app launch. Meeting loads its
/// CPU-only context inside its blocking transcription task.
///
/// `whisper_busy` enforces WP-71's mutual exclusion between Meeting
/// transcription and Streaming capture via `streaming_session::WhisperUsageGuard`.
/// Streaming owns the cached Metal context; Meeting loads a CPU-only context
/// for each run. It's a plain field, not wrapped alongside `model`, so
/// acquiring it stays synchronous without holding `model`'s lock.
#[derive(Default)]
pub(crate) struct AppState {
    pub(crate) model: Mutex<Option<Arc<WhisperContext>>>,
    pub(crate) whisper_busy: std::sync::atomic::AtomicU8,
    /// The running Streaming session's audio capture, present only while a
    /// session is active. Dropping it (via `stop_streaming_session` taking
    /// it out, or app shutdown dropping `AppState` itself) stops both
    /// capture streams, which cascades: the mixer thread ends, the sample
    /// channel disconnects, the decode loop ends, and the results-consuming
    /// task releases `whisper_busy` and marks the session stopped — see
    /// `docs/architecture.md`'s Streaming IPC section.
    #[cfg(target_os = "macos")]
    pub(crate) streaming_runtime: Mutex<Option<StreamingRuntime>>,
}

// Both fields are held for their effect, not read back: `session_id`
// documents which session this runtime belongs to (useful reading a debug
// dump or extending this later); `capture`'s only job is to exist until
// `stop_streaming_session` drops it, which is what actually stops capture.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub(crate) struct StreamingRuntime {
    pub(crate) session_id: i64,
    pub(crate) capture: streaming_audio::StreamingSession,
}

impl AppState {
    pub(crate) async fn model(&self, app_support_dir: PathBuf) -> Result<Arc<WhisperContext>> {
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
