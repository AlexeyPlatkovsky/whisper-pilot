//! Streaming-session IPC commands: list/open/rename/delete persisted
//! sessions, plus start/stop the live session (macOS capture).

use crate::error::{AppError, Result};
use crate::events::{StreamingSessionEndedEvent, StreamingSourcesEvent, StreamingWindowEvent};
use crate::state::{app_data_dir, now_ms, AppState, StreamingRuntime};
use crate::streaming;
use crate::streaming_audio;
use crate::streaming_session;
use crate::streaming_store;
use crate::transcribe;
use tauri::{Emitter, Manager, State};

/// List persisted Streaming sessions newest-touched first, for the
/// Streaming tab's session list.
#[tauri::command]
pub(crate) fn list_streaming_sessions(
    app: tauri::AppHandle,
) -> Result<Vec<streaming::StreamingSessionSummaryDto>> {
    streaming::list_streaming_sessions(&app_data_dir(&app)?)
}

/// Open a complete persisted Streaming session (all decoded windows) for
/// the Streaming workspace.
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

/// Runs on its own blocking thread for a session's whole lifetime: persists
/// each decoded window as it arrives (WP-72's incremental save) and emits
/// `streaming_window` so the UI updates live. When `results_rx` disconnects
/// — the decode loop ended because `stop_streaming_session` dropped the
/// capture — finalizes the session: marks it stopped, releases
/// `whisper_busy` so Meeting (or a new Streaming session) can run again, and
/// emits `streaming_session_ended`.
#[cfg(target_os = "macos")]
fn drive_streaming_results(
    app: tauri::AppHandle,
    app_support_dir: std::path::PathBuf,
    session_id: i64,
    results_rx: std::sync::mpsc::Receiver<streaming_session::WindowResult>,
) {
    for result in results_rx.iter() {
        let (text, language, outcome_ok) = match &result.outcome {
            Ok(transcription) => {
                let text = transcription
                    .segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                (text, transcription.language.clone(), true)
            }
            Err(e) => {
                log::warn!(
                    "streaming session {session_id} window {} fail-open: {e}",
                    result.window_index
                );
                (
                    String::new(),
                    transcribe::UNDETECTED_LANGUAGE.to_string(),
                    false,
                )
            }
        };
        let end_ms = result.start_ms + streaming_session::WINDOW_MS;

        match streaming_store::StreamingStore::open(&app_support_dir) {
            Ok(store) => {
                let window = streaming_store::NewStreamingWindow {
                    window_index: result.window_index as i64,
                    start_ms: result.start_ms as i64,
                    end_ms: end_ms as i64,
                    text: text.clone(),
                    language: language.clone(),
                    outcome_ok,
                };
                let now = now_ms().unwrap_or(result.start_ms as i64);
                if let Err(e) = store.append_window(session_id, &window, now) {
                    log::error!(
                        "streaming session {session_id}: failed to persist window {}: {e}",
                        result.window_index
                    );
                }
            }
            Err(e) => log::error!("streaming session {session_id}: failed to open store: {e}"),
        }

        let _ = app.emit(
            "streaming_window",
            StreamingWindowEvent {
                session_id,
                window_index: result.window_index as i64,
                start_ms: result.start_ms as i64,
                end_ms: end_ms as i64,
                text,
                language,
                outcome_ok,
            },
        );
    }

    // `results_rx.iter()` ended: the decode loop returned, which only
    // happens once the sample channel disconnects, which only happens once
    // the capture's mixer thread stops, which only happens once
    // `StreamingRuntime` (holding the capture) is dropped.
    let now = now_ms().unwrap_or(0);
    if let Ok(store) = streaming_store::StreamingStore::open(&app_support_dir) {
        if let Err(e) = store.mark_stopped(session_id, now) {
            log::error!("streaming session {session_id}: failed to mark stopped: {e}");
        }
    }
    streaming_session::release_whisper_busy(&app.state::<AppState>().whisper_busy);
    let _ = app.emit(
        "streaming_session_ended",
        StreamingSessionEndedEvent { session_id },
    );
}

/// Start a Streaming session: claims the shared Whisper context (mutually
/// exclusive with an active Meeting transcription, WP-71), creates the
/// session's DB record, starts audio capture (mic + system-audio, degrading
/// to whichever source(s) actually came up), and spawns the decode and
/// persistence loops. Returns once capture has started — decoding continues
/// in the background; the caller listens for `streaming_window` events.
#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn start_streaming_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: Option<streaming_store::StreamingSessionId>,
) -> Result<streaming::StreamingSessionSummaryDto> {
    if state.streaming_runtime.lock().await.is_some() {
        return Err(AppError::Capture(
            "a Streaming session is already running".into(),
        ));
    }
    streaming_session::try_claim_streaming(&state.whisper_busy).map_err(|holder| {
        AppError::Capture(match holder {
            streaming_session::WhisperUser::Meeting => {
                "a meeting is currently transcribing; stop it before starting a Streaming session"
                    .to_string()
            }
            streaming_session::WhisperUser::Streaming => {
                "a Streaming session is already running".to_string()
            }
        })
    })?;

    let app_support_dir = app_data_dir(&app)?;

    let ctx = match state.model(app_support_dir.clone()).await {
        Ok(ctx) => ctx,
        Err(e) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(e);
        }
    };

    let now = now_ms()?;
    // A `session_id` continues an existing, previously-stopped session
    // ("Start" on an open past session resumes it rather than always
    // spawning a new one) — `starting_window_index` picks up window
    // numbering where that session left off; the fresh-session branch always
    // starts at 0.
    let (summary, starting_window_index) = match session_id {
        Some(id) => match streaming::resume_streaming_session(&app_support_dir, id, now) {
            Ok(result) => result,
            Err(e) => {
                streaming_session::release_whisper_busy(&state.whisper_busy);
                return Err(e);
            }
        },
        None => match streaming::create_streaming_session(&app_support_dir, now) {
            Ok(id) => (
                streaming::StreamingSessionSummaryDto {
                    id,
                    title: "New Streaming Session".to_string(),
                    created_at_ms: now,
                    updated_at_ms: now,
                    status: streaming_store::status::ACTIVE.to_string(),
                },
                0,
            ),
            Err(e) => {
                streaming_session::release_whisper_busy(&state.whisper_busy);
                return Err(e);
            }
        },
    };
    let session_id = summary.id;

    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    let capture = match streaming_audio::StreamingSession::start(samples_tx) {
        Ok(capture) => capture,
        Err(e) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            // The session record already exists; leave it — an empty,
            // never-started session is a legitimate (if unfortunate) history
            // entry, consistent with a Meeting whose transcription failed.
            return Err(e);
        }
    };
    let active_sources = capture.active_sources();

    let (results_tx, results_rx) = std::sync::mpsc::channel();
    tokio::task::spawn_blocking(move || {
        streaming_session::run_windowed_decode(
            move || streaming_session::WhisperSessionDecoder::new(&ctx),
            samples_rx,
            results_tx,
            starting_window_index,
        )
    });

    let results_app = app.clone();
    let results_dir = app_support_dir.clone();
    tokio::task::spawn_blocking(move || {
        drive_streaming_results(results_app, results_dir, session_id, results_rx)
    });

    *state.streaming_runtime.lock().await = Some(StreamingRuntime {
        session_id,
        capture,
    });

    let _ = app.emit(
        "streaming_sources",
        StreamingSourcesEvent {
            session_id,
            mic: matches!(
                active_sources,
                streaming_audio::ActiveSources::Both | streaming_audio::ActiveSources::MicOnly
            ),
            system_audio: matches!(
                active_sources,
                streaming_audio::ActiveSources::Both | streaming_audio::ActiveSources::SystemAudioOnly
            ),
        },
    );

    Ok(summary)
}

/// Stop the running Streaming session. Dropping the held capture stops both
/// audio sources; `drive_streaming_results` finishes persisting/emitting on
/// its own once the resulting sample/decode-loop disconnect cascades
/// through, releasing `whisper_busy` and marking the session stopped.
#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn stop_streaming_session(state: State<'_, AppState>) -> Result<()> {
    let runtime = state.streaming_runtime.lock().await.take();
    if runtime.is_none() {
        return Err(AppError::Capture("no Streaming session is running".into()));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn start_streaming_session(
    _state: State<'_, AppState>,
    _session_id: Option<streaming_store::StreamingSessionId>,
) -> Result<streaming::StreamingSessionSummaryDto> {
    Err(AppError::Capture(
        "Streaming's audio capture is only available on macOS".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn stop_streaming_session(_state: State<'_, AppState>) -> Result<()> {
    Err(AppError::Capture(
        "Streaming's audio capture is only available on macOS".into(),
    ))
}
