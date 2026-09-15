//! Streaming-session IPC commands: list/open/rename/delete persisted
//! sessions, plus start/stop the live session (macOS capture).

use crate::error::{AppError, Result};
#[cfg(target_os = "macos")]
use crate::events::{
    StreamingErrorEvent, StreamingPartialEvent, StreamingSourcesEvent, StreamingWindowEvent,
};
use crate::live_capture::LiveCaptureSource;
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

#[cfg(target_os = "macos")]
#[path = "streaming_cloud_results.rs"]
mod streaming_cloud_results;
#[path = "streaming_library.rs"]
pub(crate) mod streaming_library;
#[path = "streaming_lifecycle.rs"]
pub(crate) mod streaming_lifecycle;

#[cfg(target_os = "macos")]
use streaming_cloud_results::drive_cloud_results;
#[cfg(target_os = "macos")]
use streaming_lifecycle::{
    begin_live_capture, emit_live_capture_state, fail_running_capture,
    fail_start_after_status_cleanup, finish_cancelled_start, finish_persisted_session,
    require_current_start,
};

#[cfg(target_os = "macos")]
// At the nominal 100 ms cadence, a full minute of bounded backlog uses about
// 3.84 MB at 16 kHz mono f32 (5.76 MB at 24 kHz) while absorbing first-load
// and committed-translation Metal stalls. The old 3.2-second queue
// irreversibly dropped audio whenever Whisper shared the GPU with a quality LLM.
const AUDIO_CHUNK_QUEUE_CAPACITY: usize = 600;

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
    results_rx: streaming_session::ResultReceiver,
) {
    // Keep one configured SQLite connection for this local capture's entire
    // result lifetime. Reopening here used to rerun schema setup for every
    // committed window and made normal concurrent UI reads more likely to
    // contend with a writer.
    let store = match streaming_store::StreamingStore::open(&app_support_dir) {
        Ok(store) => store,
        Err(error) => {
            let message =
                "Meeting stopped because transcript storage could not be opened.".to_string();
            log::error!("streaming session {session_id}: {message}: {error}");
            let _ = app.emit(
                "streaming_error",
                StreamingErrorEvent {
                    session_id,
                    message: message.clone(),
                },
            );
            fail_running_capture(&app, generation, message);
            streaming_session::release_whisper_busy(&app.state::<AppState>().whisper_busy);
            let _ = finish_persisted_session(&app, &app_support_dir, session_id, generation, false);
            return;
        }
    };
    let mut terminal_error = false;
    while let Ok(result) = results_rx.recv() {
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
                .unwrap_or_else(|| "Meeting transcript contains an audio gap.".to_string());
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

        let window = streaming_store::NewStreamingWindow {
            window_index: result.window_index as i64,
            start_ms: result.start_ms as i64,
            end_ms: result.end_ms as i64,
            text: text.clone(),
            language: language.clone(),
            outcome_ok,
        };
        let now = now_ms().unwrap_or(result.start_ms as i64);
        let persistence_error = store
            .append_window_with_retry(session_id, &window, now)
            .err()
            .map(|error| error.to_string());
        if let Some(detail) = persistence_error {
            log::error!(
                "streaming session {session_id}: failed to persist window {}: {detail}",
                result.window_index
            );
            let message =
                "Meeting stopped because a transcript window could not be saved.".to_string();
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

    if let Some(queue_error) = results_rx.terminal_error() {
        let message = queue_error.message().to_string();
        log::error!("streaming session {session_id}: {message}");
        let _ = app.emit(
            "streaming_error",
            StreamingErrorEvent {
                session_id,
                message: message.clone(),
            },
        );
        fail_running_capture(&app, generation, message);
        terminal_error = true;
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
/// Start a Streaming session: claims the shared Whisper context (mutually
/// exclusive with an active Meeting transcription, WP-71), creates the
/// session's DB record, starts system-audio capture, and spawns the decode and
/// persistence loops. Returns once capture has started — decoding continues
/// in the background; the caller listens for `streaming_window` events.
#[cfg(target_os = "macos")]
enum LocalStreamingDecoderModel {
    Whisper(std::sync::Arc<whisper_rs::WhisperContext>),
    QwenGguf(std::sync::Arc<crate::qwen_gguf_asr::QwenGgufAsrModel>),
}

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
                "unknown Meeting engine".to_string(),
            ))
        }
    };
    // Hold the same mutation barrier as Recorder while resolving the local
    // selection and claiming the shared ASR runtime. Without this span a
    // model-delete/settings mutation could observe an idle decoder between
    // selection and the busy claim, then remove the bundle being started.
    let local_asr_mutation = matches!(requested, streaming::StreamingStartConfiguration::Local)
        .then(|| state.recorder_asr_mutation.lock());
    let local_asr_mutation = match local_asr_mutation {
        Some(lock) => Some(lock.await),
        None => None,
    };
    let local_asr_spec = if local_asr_mutation.is_some() {
        let app_settings = crate::settings::get_settings(&app_support_dir);
        let model_id = app_settings
            .active_model_transcription
            .as_deref()
            .unwrap_or(crate::asr::DEFAULT_ASR_MODEL_ID);
        Some(crate::asr::resolve_selection(
            model_id,
            crate::asr::AsrMode::Streaming,
            crate::asr::AsrLanguage::Auto,
        )?)
    } else {
        None
    };
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
                "a file transcription is currently running; wait before starting a Meeting"
                    .to_string()
            }
            streaming_session::WhisperUser::Streaming => "a Meeting is already running".to_string(),
            streaming_session::WhisperUser::Recorder => {
                "Recorder is currently capturing; stop it before starting a Meeting".to_string()
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
    // Successful ownership means a mutation can now see `whisper_busy` and
    // reject safely; do not serialize model load/capture startup behind it.
    drop(local_asr_mutation);

    let asr_spec = local_asr_spec.expect("local Streaming has a resolved ASR model");
    let decoder_model = match asr_spec.runtime {
        crate::asr::AsrRuntime::WhisperCpp => state
            .model(app_support_dir.clone())
            .await
            .map(LocalStreamingDecoderModel::Whisper),
        crate::asr::AsrRuntime::LlamaCppMtmd => state
            .qwen_gguf_asr_model(app_support_dir.clone(), asr_spec)
            .await
            .map(LocalStreamingDecoderModel::QwenGguf),
    };
    let decoder_model = match decoder_model {
        Ok(model) => model,
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
    tokio::task::spawn_blocking(move || match decoder_model {
        LocalStreamingDecoderModel::Whisper(ctx) => streaming_session::run_windowed_decode_from(
            move || streaming_session::WhisperSessionDecoder::new(&ctx),
            samples_rx,
            results_tx,
            resume.next_window_index,
            resume.last_persisted_end_ms,
        ),
        LocalStreamingDecoderModel::QwenGguf(model) => streaming_session::run_windowed_decode_from(
            move || Ok(streaming_session::QwenGgufSessionDecoder::new(model)),
            samples_rx,
            results_tx,
            resume.next_window_index,
            resume.last_persisted_end_ms,
        ),
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::AUDIO_CHUNK_QUEUE_CAPACITY;
    use crate::streaming_audio::{try_send_drop_newest, CapturedAudioChunk, QueueSendOutcome};

    #[test]
    fn local_audio_queue_holds_sixty_seconds_then_drops_the_next_100ms_chunk() {
        let (sender, _receiver) = std::sync::mpsc::sync_channel(AUDIO_CHUNK_QUEUE_CAPACITY);

        for chunk_index in 0..AUDIO_CHUNK_QUEUE_CAPACITY as u64 {
            assert_eq!(
                try_send_drop_newest(
                    &sender,
                    CapturedAudioChunk {
                        start_sample: chunk_index * 1_600,
                        captured_end_sample: (chunk_index + 1) * 1_600,
                        samples: vec![0.0; 1_600],
                    },
                ),
                QueueSendOutcome::Sent,
                "100ms chunk {chunk_index} must survive the bounded local decode backlog",
            );
        }

        let overflow_index = AUDIO_CHUNK_QUEUE_CAPACITY as u64;
        assert_eq!(
            try_send_drop_newest(
                &sender,
                CapturedAudioChunk {
                    start_sample: overflow_index * 1_600,
                    captured_end_sample: (overflow_index + 1) * 1_600,
                    samples: vec![0.0; 1_600],
                },
            ),
            QueueSendOutcome::DroppedNewest,
        );
    }
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn start_streaming_session(
    _state: State<'_, AppState>,
    _session_id: Option<streaming_store::StreamingSessionId>,
    _engine: Option<String>,
) -> Result<streaming::StreamingSessionSummaryDto> {
    Err(AppError::Capture(
        "Meeting audio capture is only available on macOS".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn stop_streaming_session(_state: State<'_, AppState>) -> Result<()> {
    Err(AppError::Capture(
        "Meeting audio capture is only available on macOS".into(),
    ))
}
