//! Shared live-capture lifecycle transitions for Meeting IPC commands.
//!
//! Keeping these transitions outside the command registration module makes
//! the long-running local and Cloud result drivers easier to audit without
//! changing their public IPC surface.

use crate::error::{AppError, Result};
use crate::events::LiveCaptureStateEvent;
#[cfg(target_os = "macos")]
use crate::events::{StreamingErrorEvent, StreamingSessionEndedEvent};
use crate::live_capture::{LiveCaptureSnapshot, LiveCaptureSource};
use crate::state::{now_ms, AppState};
#[cfg(target_os = "macos")]
use crate::streaming_store;
use tauri::State;
#[cfg(target_os = "macos")]
use tauri::{Emitter, Manager};

const LIVE_CAPTURE_STATE_EVENT: &str = "live_capture_state";

pub(super) fn emit_live_capture_state(app: &tauri::AppHandle, snapshot: LiveCaptureStateEvent) {
    let _ = app.emit(LIVE_CAPTURE_STATE_EVENT, snapshot);
}

fn live_capture_snapshot(state: &AppState) -> Result<LiveCaptureSnapshot> {
    state
        .live_capture
        .lock()
        .map(|coordinator| coordinator.snapshot())
        .map_err(|_| AppError::Capture("live capture coordinator lock is poisoned".into()))
}

#[cfg(target_os = "macos")]
pub(super) fn require_current_start(state: &AppState, generation: u64) -> Result<()> {
    let snapshot = live_capture_snapshot(state)?;
    if snapshot.generation == generation
        && snapshot.phase == crate::live_capture::LiveCapturePhase::Starting
    {
        Ok(())
    } else {
        Err(AppError::Capture("live capture start was cancelled".into()))
    }
}

#[tauri::command]
pub(crate) fn get_live_capture_snapshot(state: State<'_, AppState>) -> Result<LiveCaptureSnapshot> {
    live_capture_snapshot(&state)
}

#[cfg(target_os = "macos")]
pub(super) fn begin_live_capture(
    app: &tauri::AppHandle,
    state: &AppState,
    session_id: i64,
) -> Result<u64> {
    let snapshot = state
        .live_capture
        .lock()
        .map_err(|_| AppError::Capture("live capture coordinator lock is poisoned".into()))?
        .begin_start(session_id, LiveCaptureSource::Streaming)
        .map_err(|error| AppError::Capture(error.to_string()))?;
    let generation = snapshot.generation;
    emit_live_capture_state(app, snapshot);
    Ok(generation)
}

#[cfg(target_os = "macos")]
pub(super) fn fail_live_capture<T>(
    app: &tauri::AppHandle,
    state: &AppState,
    generation: u64,
    error: AppError,
) -> Result<T> {
    let transition = state
        .live_capture
        .lock()
        .map_err(|_| AppError::Capture("live capture coordinator lock is poisoned".into()))?
        .fail(generation, error.to_string());
    if let Ok((snapshot, runtime)) = transition {
        emit_live_capture_state(app, snapshot);
        drop(runtime);
    }
    Err(error)
}

#[cfg(target_os = "macos")]
pub(super) fn finish_live_capture(app: &tauri::AppHandle, generation: u64) {
    let transition = app
        .state::<AppState>()
        .live_capture
        .lock()
        .ok()
        .and_then(|mut coordinator| coordinator.finish_stop(generation).ok());
    if let Some(snapshot) = transition {
        emit_live_capture_state(app, snapshot);
    }
}

#[cfg(target_os = "macos")]
pub(super) fn fail_running_capture(app: &tauri::AppHandle, generation: u64, message: String) {
    let runtime = app
        .state::<AppState>()
        .live_capture
        .lock()
        .ok()
        .and_then(|mut coordinator| {
            coordinator
                .fail(generation, message)
                .ok()
                .map(|(snapshot, runtime)| {
                    emit_live_capture_state(app, snapshot);
                    runtime
                })
        })
        .flatten();
    drop(runtime);
}

#[cfg(target_os = "macos")]
fn mark_session_stopped(
    app_support_dir: &std::path::Path,
    session_id: i64,
    now: i64,
) -> Result<()> {
    streaming_store::StreamingStore::open(app_support_dir)?.mark_stopped(session_id, now)
}

#[cfg(target_os = "macos")]
pub(super) fn fail_start_after_status_cleanup<T>(
    app: &tauri::AppHandle,
    state: &AppState,
    app_support_dir: &std::path::Path,
    session_id: i64,
    generation: u64,
    now: i64,
    original_error: AppError,
) -> Result<T> {
    let error = match mark_session_stopped(app_support_dir, session_id, now) {
        Ok(()) => original_error,
        Err(error) => AppError::Store(format!(
            "Meeting failed and its status could not be saved: {error}"
        )),
    };
    fail_live_capture(app, state, generation, error)
}

#[cfg(target_os = "macos")]
pub(super) fn finish_cancelled_start<T>(
    app: &tauri::AppHandle,
    state: &AppState,
    app_support_dir: &std::path::Path,
    session_id: i64,
    generation: u64,
    cancellation: AppError,
) -> Result<T> {
    let now = match now_ms() {
        Ok(now) => now,
        Err(error) => return fail_live_capture(app, state, generation, error),
    };
    if let Err(error) = mark_session_stopped(app_support_dir, session_id, now) {
        return fail_live_capture(
            app,
            state,
            generation,
            AppError::Store(format!(
                "Meeting was cancelled but its status could not be saved: {error}"
            )),
        );
    }
    finish_live_capture(app, generation);
    Err(cancellation)
}

#[cfg(target_os = "macos")]
pub(super) fn finish_persisted_session(
    app: &tauri::AppHandle,
    app_support_dir: &std::path::Path,
    session_id: i64,
    generation: u64,
    successful: bool,
) -> bool {
    let persistence =
        now_ms().and_then(|now| mark_session_stopped(app_support_dir, session_id, now));
    if let Err(error) = persistence {
        log::error!("streaming session {session_id}: failed to mark stopped: {error}");
        let message = "Meeting stopped, but its final status could not be saved.".to_string();
        let _ = app.emit(
            "streaming_error",
            StreamingErrorEvent {
                session_id,
                message: message.clone(),
            },
        );
        fail_running_capture(app, generation, message);
        return false;
    }
    if successful {
        finish_live_capture(app, generation);
        let _ = app.emit(
            "streaming_session_ended",
            StreamingSessionEndedEvent { session_id },
        );
    }
    true
}
