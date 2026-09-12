//! Streaming-session IPC commands: list/open/rename/delete persisted
//! sessions, plus start/stop the live session (macOS capture).

use crate::error::{AppError, Result};
use crate::events::LiveCaptureStateEvent;
#[cfg(target_os = "macos")]
use crate::events::{
    StreamingErrorEvent, StreamingPartialEvent, StreamingSessionEndedEvent, StreamingSourcesEvent,
    StreamingWindowEvent,
};
use crate::live_capture::{LiveCaptureSnapshot, LiveCaptureSource};
use crate::state::{app_data_dir, now_ms, AppState};
#[cfg(target_os = "macos")]
use crate::state::{LiveCaptureRuntime, StreamingRuntime};
use crate::streaming;
#[cfg(target_os = "macos")]
use crate::streaming_audio;
#[cfg(target_os = "macos")]
use crate::streaming_session;
use crate::streaming_store;
#[cfg(target_os = "macos")]
use crate::transcribe;
#[cfg(target_os = "macos")]
use crate::{
    cloud_provider::{CloudProvider, KeychainCredentialStore},
    cloud_streaming::{CloudStreamingResult, CloudTransport},
};
use tauri::State;
#[cfg(target_os = "macos")]
use tauri::{Emitter, Manager};

const LIVE_CAPTURE_STATE_EVENT: &str = "live_capture_state";
#[cfg(target_os = "macos")]
const AUDIO_CHUNK_QUEUE_CAPACITY: usize = 32;

fn emit_live_capture_state(app: &tauri::AppHandle, snapshot: LiveCaptureStateEvent) {
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
fn require_current_start(state: &AppState, generation: u64) -> Result<()> {
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
fn begin_live_capture(app: &tauri::AppHandle, state: &AppState, session_id: i64) -> Result<u64> {
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
fn fail_live_capture<T>(
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
fn finish_live_capture(app: &tauri::AppHandle, generation: u64) {
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
fn fail_running_capture(app: &tauri::AppHandle, generation: u64, message: String) {
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
fn fail_start_after_status_cleanup<T>(
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
            "Streaming failed and its session status could not be saved: {error}"
        )),
    };
    fail_live_capture(app, state, generation, error)
}

#[cfg(target_os = "macos")]
fn finish_cancelled_start<T>(
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
                "Streaming was cancelled but its session status could not be saved: {error}"
            )),
        );
    }
    finish_live_capture(app, generation);
    Err(cancellation)
}

#[cfg(target_os = "macos")]
fn finish_persisted_session(
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
        let message =
            "Streaming stopped, but its final session status could not be saved.".to_string();
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

/// All persisted window translations for one session and target language
/// (WP-93) — read counterpart to `translate_streaming_window`, so the
/// frontend can reuse an already-translated window instead of re-running
/// the model.
#[tauri::command]
pub(crate) fn list_streaming_translations(
    app: tauri::AppHandle,
    session_id: streaming_store::StreamingSessionId,
    target_language: String,
) -> Result<Vec<streaming::StreamingTranslationDto>> {
    streaming::list_streaming_translations(&app_data_dir(&app)?, session_id, &target_language)
}

/// Create a stopped Streaming session record. Capture begins only when the
/// user subsequently invokes `start_streaming_session` for this session.
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
        status: session.status,
        translation_enabled: session.translation_enabled,
    })
}

/// Persists the Live Translation on/off choice for one session (WP-101) —
/// best-effort from the front-end's perspective (`src/ipc.ts`'s
/// `setStreamingTranslationEnabled`): a write failure here surfaces as a
/// rejected promise the caller swallows, matching WP-96's MFU-panel toggle
/// pattern rather than blocking or reverting the switch.
#[tauri::command]
pub(crate) fn set_streaming_translation_enabled(
    app: tauri::AppHandle,
    session_id: streaming_store::StreamingSessionId,
    enabled: bool,
) -> Result<()> {
    streaming::set_streaming_translation_enabled(&app_data_dir(&app)?, session_id, enabled)
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
    generation: u64,
    results_rx: std::sync::mpsc::Receiver<streaming_session::WindowResult>,
) {
    let mut terminal_error = false;
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
        if result.kind == streaming_session::WindowResultKind::Gap {
            let message = result
                .outcome
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_else(|| "Streaming transcript contains an audio gap.".to_string());
            let _ = app.emit(
                "streaming_error",
                StreamingErrorEvent {
                    session_id,
                    message,
                },
            );
        }
        if result.kind == streaming_session::WindowResultKind::Partial {
            let _ = app.emit(
                "streaming_partial",
                StreamingPartialEvent {
                    session_id,
                    item_id: None,
                    text,
                },
            );
            continue;
        }

        let persistence_error = match streaming_store::StreamingStore::open(&app_support_dir) {
            Ok(store) => {
                let window = streaming_store::NewStreamingWindow {
                    window_index: result.window_index as i64,
                    start_ms: result.start_ms as i64,
                    end_ms: result.end_ms as i64,
                    text: text.clone(),
                    language: language.clone(),
                    outcome_ok,
                };
                let now = now_ms().unwrap_or(result.start_ms as i64);
                if let Err(e) = store.append_window(session_id, &window, now) {
                    Some(e.to_string())
                } else {
                    None
                }
            }
            Err(e) => Some(e.to_string()),
        };
        if let Some(detail) = persistence_error {
            log::error!(
                "streaming session {session_id}: failed to persist window {}: {detail}",
                result.window_index
            );
            let message =
                "Streaming stopped because a transcript window could not be saved.".to_string();
            let _ = app.emit(
                "streaming_error",
                StreamingErrorEvent {
                    session_id,
                    message: message.clone(),
                },
            );
            fail_running_capture(&app, generation, message);
            terminal_error = true;
            break;
        }

        let _ = app.emit(
            "streaming_window",
            StreamingWindowEvent {
                session_id,
                item_id: None,
                window_index: result.window_index as i64,
                start_ms: result.start_ms as i64,
                end_ms: result.end_ms as i64,
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
    streaming_session::release_whisper_busy(&app.state::<AppState>().whisper_busy);
    finish_persisted_session(
        &app,
        &app_support_dir,
        session_id,
        generation,
        !terminal_error,
    );
}

#[cfg(target_os = "macos")]
fn drive_cloud_results(
    app: tauri::AppHandle,
    app_support_dir: std::path::PathBuf,
    session_id: i64,
    generation: u64,
    starting_window_index: u64,
    timeline_offset_ms: u64,
    results_rx: tokio::sync::mpsc::Receiver<CloudStreamingResult>,
) {
    let mut results_rx = results_rx;
    let mut next_window_index = starting_window_index as i64;
    let timeline_offset_ms = timeline_offset_ms.min(i64::MAX as u64) as i64;
    let mut previous_end_ms = timeline_offset_ms;
    let mut terminal_error = false;

    while let Some(result) = results_rx.blocking_recv() {
        match result {
            CloudStreamingResult::Partial { item_id, text } => {
                let _ = app.emit(
                    "streaming_partial",
                    StreamingPartialEvent {
                        session_id,
                        item_id,
                        text,
                    },
                );
            }
            CloudStreamingResult::Final {
                item_id,
                text,
                language,
                end_ms,
            } => {
                let (start_ms, end_ms) = streaming::resumed_cloud_window_bounds(
                    timeline_offset_ms,
                    previous_end_ms,
                    end_ms,
                );
                let window = streaming_store::NewStreamingWindow {
                    window_index: next_window_index,
                    start_ms,
                    end_ms,
                    text: text.clone(),
                    language: language.clone(),
                    outcome_ok: true,
                };
                let persistence_result = streaming_store::StreamingStore::open(&app_support_dir)
                    .and_then(|store| {
                        store.append_window(session_id, &window, now_ms().unwrap_or(end_ms))
                    });
                if let Err(error) = persistence_result {
                    log::error!("streaming session {session_id}: failed to persist Cloud transcript: {error}");
                    let message =
                        "Streaming stopped because a transcript window could not be saved."
                            .to_string();
                    let _ = app.emit(
                        "streaming_error",
                        StreamingErrorEvent {
                            session_id,
                            message: message.clone(),
                        },
                    );
                    fail_running_capture(&app, generation, message);
                    terminal_error = true;
                    break;
                }
                let _ = app.emit(
                    "streaming_window",
                    StreamingWindowEvent {
                        session_id,
                        item_id,
                        window_index: next_window_index,
                        start_ms,
                        end_ms,
                        text,
                        language,
                        outcome_ok: true,
                    },
                );
                next_window_index += 1;
                previous_end_ms = end_ms;
            }
            CloudStreamingResult::Degraded {
                start_ms,
                end_ms,
                message,
            } => {
                let start_ms = timeline_offset_ms
                    .saturating_add(start_ms.max(0))
                    .max(previous_end_ms);
                let end_ms = timeline_offset_ms
                    .saturating_add(end_ms.max(0))
                    .max(start_ms.saturating_add(1));
                let window = streaming_store::NewStreamingWindow {
                    window_index: next_window_index,
                    start_ms,
                    end_ms,
                    text: String::new(),
                    language: transcribe::UNDETECTED_LANGUAGE.to_string(),
                    outcome_ok: false,
                };
                if let Err(error) = streaming_store::StreamingStore::open(&app_support_dir)
                    .and_then(|store| {
                        store.append_window(session_id, &window, now_ms().unwrap_or(end_ms))
                    })
                {
                    log::error!("streaming session {session_id}: failed to persist Cloud overload gap: {error}");
                    let persistence_message =
                        "Streaming stopped because an overload gap could not be saved.".to_string();
                    let _ = app.emit(
                        "streaming_error",
                        StreamingErrorEvent {
                            session_id,
                            message: persistence_message.clone(),
                        },
                    );
                    fail_running_capture(&app, generation, persistence_message);
                    terminal_error = true;
                    break;
                }
                let _ = app.emit(
                    "streaming_error",
                    StreamingErrorEvent {
                        session_id,
                        message: message.clone(),
                    },
                );
                let _ = app.emit(
                    "streaming_window",
                    StreamingWindowEvent {
                        session_id,
                        item_id: None,
                        window_index: next_window_index,
                        start_ms,
                        end_ms,
                        text: String::new(),
                        language: transcribe::UNDETECTED_LANGUAGE.to_string(),
                        outcome_ok: false,
                    },
                );
                fail_running_capture(&app, generation, message);
                terminal_error = true;
                break;
            }
            CloudStreamingResult::Failed { message } => {
                let _ = app.emit(
                    "streaming_error",
                    StreamingErrorEvent {
                        session_id,
                        message: message.clone(),
                    },
                );
                fail_running_capture(&app, generation, message);
                terminal_error = true;
                break;
            }
        }
    }

    finish_persisted_session(
        &app,
        &app_support_dir,
        session_id,
        generation,
        !terminal_error,
    );
}

/// Start a Streaming session: claims the shared Whisper context (mutually
/// exclusive with an active Meeting transcription, WP-71), creates the
/// session's DB record, starts system-audio capture, and spawns the decode and
/// persistence loops. Returns once capture has started — decoding continues
/// in the background; the caller listens for `streaming_window` events.
#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn start_streaming_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: Option<streaming_store::StreamingSessionId>,
    engine: Option<String>,
) -> Result<streaming::StreamingSessionSummaryDto> {
    let app_support_dir = app_data_dir(&app)?;
    let requested = match engine.as_deref().unwrap_or("local") {
        "local" => streaming::StreamingStartConfiguration::Local,
        "cloud" => {
            let provider = CloudProvider::try_from(
                crate::settings::get_settings(&app_support_dir)
                    .cloud_provider
                    .as_str(),
            )?;
            streaming::StreamingStartConfiguration::Cloud(provider)
        }
        _ => {
            return Err(AppError::InvalidSetting(
                "unknown Streaming engine".to_string(),
            ))
        }
    };
    if matches!(requested, streaming::StreamingStartConfiguration::Local) {
        let app_settings = crate::settings::get_settings(&app_support_dir);
        let model_id = app_settings
            .active_model_transcription
            .as_deref()
            .unwrap_or(crate::asr::DEFAULT_ASR_MODEL_ID);
        crate::asr::resolve_selection(
            model_id,
            crate::asr::AsrMode::Streaming,
            crate::asr::AsrLanguage::Auto,
        )?;
    }
    let now = now_ms()?;
    let (id, created_for_start) = match session_id {
        Some(id) => (id, false),
        None => (
            streaming::create_streaming_session(&app_support_dir, now)?,
            true,
        ),
    };
    let generation = match begin_live_capture(&app, &state, id) {
        Ok(generation) => generation,
        Err(error) => {
            if created_for_start {
                let _ = streaming::delete_streaming_session(&app_support_dir, id);
            }
            return Err(error);
        }
    };
    let (summary, resume, configuration) = match streaming::prepare_streaming_session_start(
        &app_support_dir,
        id,
        Some(requested),
        now,
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            return fail_start_after_status_cleanup(
                &app,
                &state,
                &app_support_dir,
                id,
                generation,
                now,
                error,
            )
        }
    };
    let session_id = summary.id;

    if let streaming::StreamingStartConfiguration::Cloud(provider) = configuration {
        let api_key = match KeychainCredentialStore::load_for_transport(provider) {
            Ok(api_key) => api_key,
            Err(error) => {
                return fail_start_after_status_cleanup(
                    &app,
                    &state,
                    &app_support_dir,
                    session_id,
                    generation,
                    now,
                    error,
                );
            }
        };
        let mut transport = match CloudTransport::connect(provider, &api_key).await {
            Ok(transport) => transport,
            Err(error) => {
                return fail_start_after_status_cleanup(
                    &app,
                    &state,
                    &app_support_dir,
                    session_id,
                    generation,
                    now,
                    error,
                );
            }
        };
        drop(api_key);
        if let Err(error) = require_current_start(&state, generation) {
            transport.finish().await;
            return finish_cancelled_start(
                &app,
                &state,
                &app_support_dir,
                session_id,
                generation,
                error,
            );
        }
        let capture_spec =
            match streaming_audio::system_audio_capture_spec(transport.input_sample_rate()) {
                Ok(spec) => spec,
                Err(error) => {
                    return fail_start_after_status_cleanup(
                        &app,
                        &state,
                        &app_support_dir,
                        session_id,
                        generation,
                        now,
                        error,
                    );
                }
            };
        let (samples_tx, samples_rx) = std::sync::mpsc::sync_channel(AUDIO_CHUNK_QUEUE_CAPACITY);
        let capture = match streaming_audio::StreamingSession::start(samples_tx, capture_spec) {
            Ok(capture) => capture,
            Err(error) => {
                return fail_start_after_status_cleanup(
                    &app,
                    &state,
                    &app_support_dir,
                    session_id,
                    generation,
                    now,
                    error,
                );
            }
        };
        let runtime = LiveCaptureRuntime::Streaming(StreamingRuntime {
            session_id,
            capture,
        });
        let snapshot = match state
            .live_capture
            .lock()
            .map_err(|_| AppError::Capture("live capture coordinator lock is poisoned".into()))?
            .install_runtime(generation, runtime)
        {
            Ok(snapshot) => snapshot,
            Err(runtime) => {
                drop(runtime);
                return finish_cancelled_start(
                    &app,
                    &state,
                    &app_support_dir,
                    session_id,
                    generation,
                    AppError::Capture("live capture start was cancelled".into()),
                );
            }
        };
        emit_live_capture_state(&app, snapshot);
        let (cloud_samples_tx, cloud_samples_rx) = tokio::sync::mpsc::channel(128);
        std::thread::spawn(move || {
            for samples in samples_rx {
                if cloud_samples_tx.blocking_send(samples).is_err() {
                    break;
                }
            }
        });
        let (results_tx, results_rx) = tokio::sync::mpsc::channel(64);
        tauri::async_runtime::spawn(async move {
            if let Err(error) = transport.run(cloud_samples_rx, results_tx.clone()).await {
                let _ = results_tx
                    .send(CloudStreamingResult::Failed {
                        message: error.to_string(),
                    })
                    .await;
            }
        });
        let results_app = app.clone();
        let results_dir = app_support_dir.clone();
        tokio::task::spawn_blocking(move || {
            drive_cloud_results(
                results_app,
                results_dir,
                session_id,
                generation,
                resume.next_window_index,
                resume.last_persisted_end_ms,
                results_rx,
            )
        });
        emit_streaming_sources(&app, session_id, capture_spec);
        return Ok(summary);
    }

    let capture_spec = match streaming_audio::system_audio_capture_spec(crate::audio::SAMPLE_RATE) {
        Ok(spec) => spec,
        Err(error) => {
            return fail_start_after_status_cleanup(
                &app,
                &state,
                &app_support_dir,
                session_id,
                generation,
                now,
                error,
            );
        }
    };

    if let Err(holder) = streaming_session::try_claim_streaming(&state.whisper_busy) {
        let error = AppError::Capture(match holder {
            streaming_session::WhisperUser::Meeting => {
                "a meeting is currently transcribing; stop it before starting a Streaming session"
                    .to_string()
            }
            streaming_session::WhisperUser::Streaming => {
                "a Streaming session is already running".to_string()
            }
            streaming_session::WhisperUser::Recorder => {
                "Recorder is currently capturing; stop it before starting Streaming".to_string()
            }
        });
        return fail_start_after_status_cleanup(
            &app,
            &state,
            &app_support_dir,
            session_id,
            generation,
            now,
            error,
        );
    }

    let ctx = match state.model(app_support_dir.clone()).await {
        Ok(ctx) => ctx,
        Err(e) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return fail_start_after_status_cleanup(
                &app,
                &state,
                &app_support_dir,
                session_id,
                generation,
                now,
                e,
            );
        }
    };
    if let Err(error) = require_current_start(&state, generation) {
        streaming_session::release_whisper_busy(&state.whisper_busy);
        return finish_cancelled_start(
            &app,
            &state,
            &app_support_dir,
            session_id,
            generation,
            error,
        );
    }

    let (samples_tx, samples_rx) = std::sync::mpsc::sync_channel(AUDIO_CHUNK_QUEUE_CAPACITY);
    let capture = match streaming_audio::StreamingSession::start(samples_tx, capture_spec) {
        Ok(capture) => capture,
        Err(e) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return fail_start_after_status_cleanup(
                &app,
                &state,
                &app_support_dir,
                session_id,
                generation,
                now,
                e,
            );
        }
    };
    let runtime = LiveCaptureRuntime::Streaming(StreamingRuntime {
        session_id,
        capture,
    });
    let snapshot = match state
        .live_capture
        .lock()
        .map_err(|_| AppError::Capture("live capture coordinator lock is poisoned".into()))?
        .install_runtime(generation, runtime)
    {
        Ok(snapshot) => snapshot,
        Err(runtime) => {
            drop(runtime);
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return finish_cancelled_start(
                &app,
                &state,
                &app_support_dir,
                session_id,
                generation,
                AppError::Capture("live capture start was cancelled".into()),
            );
        }
    };
    emit_live_capture_state(&app, snapshot);
    let (results_tx, results_rx) = streaming_session::result_channel();
    tokio::task::spawn_blocking(move || {
        streaming_session::run_windowed_decode_from(
            move || streaming_session::WhisperSessionDecoder::new(&ctx),
            samples_rx,
            results_tx,
            resume.next_window_index,
            resume.last_persisted_end_ms,
        )
    });

    let results_app = app.clone();
    let results_dir = app_support_dir.clone();
    tokio::task::spawn_blocking(move || {
        drive_streaming_results(results_app, results_dir, session_id, generation, results_rx)
    });

    emit_streaming_sources(&app, session_id, capture_spec);

    Ok(summary)
}

#[cfg(target_os = "macos")]
fn emit_streaming_sources(
    app: &tauri::AppHandle,
    session_id: i64,
    capture_spec: streaming_audio::StreamingCaptureSpec,
) {
    let _ = app.emit(
        "streaming_sources",
        StreamingSourcesEvent {
            session_id,
            mic: capture_spec.microphone,
            system_audio: capture_spec.system_audio,
        },
    );
}

/// Stop the running Streaming session. Dropping the held capture stops system
/// audio; `drive_streaming_results` finishes persisting/emitting on
/// its own once the resulting sample/decode-loop disconnect cascades
/// through, releasing `whisper_busy` and marking the session stopped.
#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn stop_streaming_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let (snapshot, runtime) = {
        let mut coordinator = state
            .live_capture
            .lock()
            .map_err(|_| AppError::Capture("live capture coordinator lock is poisoned".into()))?;
        coordinator
            .begin_stop_for(LiveCaptureSource::Streaming)
            .map_err(|error| AppError::Capture(error.to_string()))?
    };
    emit_live_capture_state(&app, snapshot);
    drop(runtime);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn start_streaming_session(
    _state: State<'_, AppState>,
    _session_id: Option<streaming_store::StreamingSessionId>,
    _engine: Option<String>,
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
