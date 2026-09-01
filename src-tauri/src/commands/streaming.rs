//! Streaming-session IPC commands: list/open/rename/delete persisted
//! sessions, plus start/stop the live session (macOS capture).

use crate::error::{AppError, Result};
#[cfg(target_os = "macos")]
use crate::events::{
    StreamingErrorEvent, StreamingPartialEvent, StreamingSessionEndedEvent, StreamingSourcesEvent,
    StreamingWindowEvent,
};
#[cfg(target_os = "macos")]
use crate::state::StreamingRuntime;
use crate::state::{app_data_dir, now_ms, AppState};
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
                item_id: None,
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

#[cfg(target_os = "macos")]
fn drive_cloud_results(
    app: tauri::AppHandle,
    app_support_dir: std::path::PathBuf,
    session_id: i64,
    starting_window_index: u64,
    results_rx: tokio::sync::mpsc::Receiver<CloudStreamingResult>,
) {
    let mut results_rx = results_rx;
    let mut next_window_index = starting_window_index as i64;
    let mut previous_end_ms = 0_i64;

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
                let end_ms = end_ms.max(previous_end_ms);
                let window = streaming_store::NewStreamingWindow {
                    window_index: next_window_index,
                    start_ms: previous_end_ms,
                    end_ms,
                    text: text.clone(),
                    language: language.clone(),
                    outcome_ok: true,
                };
                if let Ok(store) = streaming_store::StreamingStore::open(&app_support_dir) {
                    if let Err(error) =
                        store.append_window(session_id, &window, now_ms().unwrap_or(end_ms))
                    {
                        log::error!("streaming session {session_id}: failed to persist Cloud transcript: {error}");
                    }
                }
                let _ = app.emit(
                    "streaming_window",
                    StreamingWindowEvent {
                        session_id,
                        item_id,
                        window_index: next_window_index,
                        start_ms: previous_end_ms,
                        end_ms,
                        text,
                        language,
                        outcome_ok: true,
                    },
                );
                next_window_index += 1;
                previous_end_ms = end_ms;
            }
            CloudStreamingResult::Failed { message } => {
                let _ = app.emit(
                    "streaming_error",
                    StreamingErrorEvent {
                        session_id,
                        message,
                    },
                );
                let app_for_stop = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_for_stop.state::<AppState>();
                    let mut runtime = state.streaming_runtime.lock().await;
                    if runtime
                        .as_ref()
                        .is_some_and(|current| current.session_id == session_id)
                    {
                        runtime.take();
                    }
                });
            }
        }
    }

    let now = now_ms().unwrap_or(0);
    if let Ok(store) = streaming_store::StreamingStore::open(&app_support_dir) {
        if let Err(error) = store.mark_stopped(session_id, now) {
            log::error!("streaming session {session_id}: failed to mark stopped: {error}");
        }
    }
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
    engine: Option<String>,
) -> Result<streaming::StreamingSessionSummaryDto> {
    if state.streaming_runtime.lock().await.is_some() {
        return Err(AppError::Capture(
            "a Streaming session is already running".into(),
        ));
    }
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
    let now = now_ms()?;
    let id = match session_id {
        Some(id) => id,
        None => streaming::create_streaming_session(&app_support_dir, now)?,
    };
    let (summary, starting_window_index, configuration) =
        streaming::prepare_streaming_session_start(&app_support_dir, id, Some(requested), now)?;
    let session_id = summary.id;

    if let streaming::StreamingStartConfiguration::Cloud(provider) = configuration {
        let api_key = match KeychainCredentialStore::load_for_transport(provider) {
            Ok(api_key) => api_key,
            Err(error) => {
                let _ = streaming_store::StreamingStore::open(&app_support_dir)
                    .and_then(|store| store.mark_stopped(session_id, now));
                return Err(error);
            }
        };
        let transport = match CloudTransport::connect(provider, &api_key).await {
            Ok(transport) => transport,
            Err(error) => {
                let _ = streaming_store::StreamingStore::open(&app_support_dir)
                    .and_then(|store| store.mark_stopped(session_id, now));
                return Err(error);
            }
        };
        drop(api_key);
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        let capture = match streaming_audio::StreamingSession::start(samples_tx) {
            Ok(capture) => capture,
            Err(error) => {
                let _ = streaming_store::StreamingStore::open(&app_support_dir)
                    .and_then(|store| store.mark_stopped(session_id, now));
                return Err(error);
            }
        };
        let active_sources = capture.active_sources();
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
                starting_window_index,
                results_rx,
            )
        });
        *state.streaming_runtime.lock().await = Some(StreamingRuntime {
            session_id,
            capture,
        });
        emit_streaming_sources(&app, session_id, active_sources);
        return Ok(summary);
    }

    if let Err(holder) = streaming_session::try_claim_streaming(&state.whisper_busy) {
        let _ = streaming_store::StreamingStore::open(&app_support_dir)
            .and_then(|store| store.mark_stopped(session_id, now));
        return Err(AppError::Capture(match holder {
            streaming_session::WhisperUser::Meeting => {
                "a meeting is currently transcribing; stop it before starting a Streaming session"
                    .to_string()
            }
            streaming_session::WhisperUser::Streaming => {
                "a Streaming session is already running".to_string()
            }
        }));
    }

    let ctx = match state.model(app_support_dir.clone()).await {
        Ok(ctx) => ctx,
        Err(e) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            let _ = streaming_store::StreamingStore::open(&app_support_dir)
                .and_then(|store| store.mark_stopped(session_id, now));
            return Err(e);
        }
    };

    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    let capture = match streaming_audio::StreamingSession::start(samples_tx) {
        Ok(capture) => capture,
        Err(e) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            let _ = streaming_store::StreamingStore::open(&app_support_dir)
                .and_then(|store| store.mark_stopped(session_id, now));
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

    emit_streaming_sources(&app, session_id, active_sources);

    Ok(summary)
}

#[cfg(target_os = "macos")]
fn emit_streaming_sources(
    app: &tauri::AppHandle,
    session_id: i64,
    active_sources: streaming_audio::ActiveSources,
) {
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
                streaming_audio::ActiveSources::Both
                    | streaming_audio::ActiveSources::SystemAudioOnly
            ),
        },
    );
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
