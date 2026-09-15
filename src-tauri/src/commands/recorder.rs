//! Recorder IPC facade and microphone-capture worker.

use super::recorder_dto::open_dto;
pub(crate) use super::recorder_dto::{RecorderSessionDto, RecorderSessionSummaryDto};
use crate::error::{AppError, Result};
use crate::recorder_audio::RecorderAudioWriter;
use crate::recorder_store::{
    RecorderSession, RecorderSessionId, RecorderStore, RecorderTranscriptUpdate,
};
use crate::state::AppState;
use std::path::{Path, PathBuf};
use tauri::State;

#[path = "recorder/library.rs"]
pub(crate) mod library;
pub(crate) use library::open_recorder_session;

#[cfg(target_os = "macos")]
#[path = "recorder/lifecycle.rs"]
mod lifecycle;
#[cfg(target_os = "macos")]
pub(crate) use lifecycle::{start_recorder_impl, stop_recorder_impl};
#[cfg(target_os = "macos")]
#[path = "recorder/integration.rs"]
mod integration;
#[cfg(target_os = "macos")]
pub(super) use integration::{begin_recorder_capture, emit_error, emit_session};
#[cfg(target_os = "macos")]
pub(crate) use integration::{handle_global_shortcut, setup_recorder};
#[path = "recorder/export.rs"]
mod export;

#[cfg(target_os = "macos")]
use crate::events::RecorderPartialEvent;
#[cfg(target_os = "macos")]
use crate::microphone_audio::{BandlimitedChunkResampler, PooledMicrophoneSamples};
#[cfg(target_os = "macos")]
use crate::recorder_audio::read_caf_audio;
#[cfg(target_os = "macos")]
use crate::streaming_audio::CapturedAudioChunk;
#[cfg(target_os = "macos")]
use crate::streaming_session;
#[cfg(target_os = "macos")]
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
#[cfg(target_os = "macos")]
use tauri::{Emitter, Manager};

#[cfg(target_os = "macos")]
pub(super) const AUDIO_QUEUE_CAPACITY: usize = 32;
#[cfg(target_os = "macos")]
pub(super) const CAPTION_WINDOW_LABEL: &str = "recorder-caption";

pub(super) fn ensure_recorder_session_is_not_live(
    app: &tauri::AppHandle,
    id: RecorderSessionId,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use crate::live_capture::{LiveCapturePhase, LiveCaptureSource};

        let snapshot = app
            .state::<AppState>()
            .live_capture
            .lock()
            .map_err(|_| AppError::Capture("live capture coordinator lock is poisoned".into()))?
            .snapshot();
        if snapshot.source == Some(LiveCaptureSource::Recorder)
            && snapshot.session_id == Some(id)
            && !matches!(
                snapshot.phase,
                LiveCapturePhase::Idle | LiveCapturePhase::Error
            )
        {
            return Err(AppError::Store(format!(
                "Recorder session {id} cannot be changed while capture is active"
            )));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn finish_recorder_capture(app: &tauri::AppHandle, generation: u64) {
    let transition = app
        .state::<AppState>()
        .live_capture
        .lock()
        .ok()
        .and_then(|mut coordinator| coordinator.finish_stop(generation).ok());
    if let Some(snapshot) = transition {
        let _ = app.emit("live_capture_state", snapshot);
    }
}

#[cfg(target_os = "macos")]
pub(super) fn fail_recorder_capture(app: &tauri::AppHandle, generation: u64, message: &str) {
    let runtime = app
        .state::<AppState>()
        .live_capture
        .lock()
        .ok()
        .and_then(|mut coordinator| coordinator.fail(generation, message.to_string()).ok())
        .and_then(|(snapshot, runtime)| {
            let _ = app.emit("live_capture_state", snapshot);
            runtime
        });
    drop(runtime);
}

#[cfg(target_os = "macos")]
struct RecorderFinalizationChannels {
    tail_persisted_tx: std::sync::mpsc::Sender<()>,
    audio_finalized_rx: Receiver<Result<PathBuf>>,
    decoder_finished_rx: Receiver<Option<String>>,
    terminal_failure_rx: Receiver<String>,
}

#[cfg(target_os = "macos")]
fn drive_recorder_results(
    app: tauri::AppHandle,
    app_support_dir: PathBuf,
    session: RecorderSession,
    generation: u64,
    results_rx: streaming_session::ResultReceiver,
    finalization: RecorderFinalizationChannels,
    quality_model: RecorderDecoderModel,
) {
    let mut terminal_error: Option<String> = None;
    // This worker owns one SQLite handle for its entire commit loop. Opening a
    // new connection for every transcript window creates avoidable schema and
    // locking work precisely while the microphone pipeline needs to keep up.
    let store = match RecorderStore::open_runtime(&app_support_dir) {
        Ok(store) => Some(store),
        Err(error) => {
            terminal_error = Some(format!(
                "Recorder transcript storage could not be opened: {error}"
            ));
            None
        }
    };
    let mut partial_revision = 0_u64;
    while let Ok(result) = results_rx.recv() {
        let (text, language) = match result.outcome {
            Ok(transcription) => (
                transcription
                    .segments
                    .iter()
                    .map(|segment| segment.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                transcription.language,
            ),
            Err(error) => {
                if recorder_result_error_is_terminal(result.kind) {
                    terminal_error = Some(error.to_string());
                }
                continue;
            }
        };
        if result.kind == streaming_session::WindowResultKind::Partial {
            partial_revision = partial_revision.saturating_add(1);
            let _ = app.emit(
                "recorder_partial",
                RecorderPartialEvent {
                    session_id: session.id,
                    revision: partial_revision,
                    text,
                },
            );
            continue;
        }
        let start_sample = result
            .start_ms
            .saturating_mul(u64::from(session.sample_rate))
            / 1_000;
        let end_sample = result.end_ms.saturating_mul(u64::from(session.sample_rate)) / 1_000;
        let update = RecorderTranscriptUpdate::Committed {
            start_sample,
            end_sample,
            text,
            language,
        };
        let saved = store
            .as_ref()
            .ok_or_else(|| AppError::Store("Recorder transcript storage is unavailable".into()))
            .and_then(|store| store.apply_transcript_update(session.id, update));
        match saved {
            Ok(Some(segment)) => {
                let _ = app.emit("recorder_segment_committed", segment);
            }
            Ok(None) => {}
            Err(error) => {
                terminal_error = Some(format!("Recorder transcript could not be saved: {error}"));
                break;
            }
        }
    }

    match finalization.decoder_finished_rx.recv() {
        Ok(Some(message)) => terminal_error = Some(message),
        Ok(None) => {}
        Err(_) => {
            terminal_error =
                Some("Recorder ASR finalizer disconnected; audio was preserved for recovery".into())
        }
    }
    for message in finalization.terminal_failure_rx {
        terminal_error.get_or_insert(message);
    }

    let _ = finalization.tail_persisted_tx.send(());
    let finalized_audio = finalization
        .audio_finalized_rx
        .recv()
        .unwrap_or_else(|_| Err(AppError::Audio("Recorder audio writer disconnected".into())));
    let finalized_path = match finalized_audio {
        Ok(path) => Some(path),
        Err(error) => {
            terminal_error = Some(format!("Recorder audio could not be finalized: {error}"));
            None
        }
    };

    if terminal_error.is_none() {
        if let Some(path) = finalized_path.as_deref() {
            if let Err(error) =
                refine_recorder_transcript(&app_support_dir, &session, &quality_model, path)
            {
                // The live transcript and finalized native-rate audio remain
                // valid. Refinement is best-effort and must not turn a usable
                // recording into a recovery failure.
                log::warn!(
                    "Recorder {} quality pass failed; keeping live transcript: {error}",
                    session.id
                );
            }
        }
    }
    match (store.as_ref(), terminal_error) {
        (Some(store), None) => {
            if let Err(error) = store.mark_completed(session.id) {
                let message = format!("Recorder final status could not be saved: {error}");
                let _ = store.mark_recoverable(session.id, &message);
                emit_error(&app, Some(session.id), message);
            }
        }
        (Some(store), Some(message)) => {
            let _ = store.mark_recoverable(session.id, &message);
            emit_error(&app, Some(session.id), message);
        }
        (None, _) => emit_error(
            &app,
            Some(session.id),
            "Recorder storage is unavailable; audio was preserved for recovery",
        ),
    }
    emit_session(&app, &app_support_dir, session.id);
    if let Some(window) = app.get_webview_window(CAPTION_WINDOW_LABEL) {
        let _ = window.hide();
    }
    finish_recorder_capture(&app, generation);
    streaming_session::release_whisper_busy(&app.state::<AppState>().whisper_busy);
}

#[cfg(target_os = "macos")]
fn refine_recorder_transcript(
    app_support_dir: &Path,
    session: &RecorderSession,
    model: &RecorderDecoderModel,
    audio_path: &Path,
) -> Result<()> {
    let audio = read_caf_audio(audio_path)?;
    let native_samples = audio
        .samples
        .iter()
        .map(|sample| f32::from(*sample) / 32_768.0)
        .collect::<Vec<_>>();
    let mut resampler =
        BandlimitedChunkResampler::new(audio.metadata.sample_rate, crate::audio::SAMPLE_RATE)
            .map_err(|error| AppError::Audio(error.to_string()))?;
    let mut samples = resampler
        .push_f32(&native_samples)
        .map_err(|error| AppError::Audio(error.to_string()))?
        .samples;
    samples.extend(
        resampler
            .finish()
            .map_err(|error| AppError::Audio(error.to_string()))?
            .samples,
    );

    let transcription = match model {
        RecorderDecoderModel::Whisper(ctx) => crate::transcribe::transcribe(ctx, &samples)?,
        RecorderDecoderModel::QwenGguf(model) => {
            crate::commands::transcription::transcribe_qwen_recording(model, &samples, |_| {})?
        }
    };
    if transcription.segments.is_empty() {
        return Err(AppError::Transcribe(
            "Recorder quality pass returned no speech".into(),
        ));
    }

    let sample_rate = u64::from(session.sample_rate);
    let frame_count = audio.metadata.frames;
    let updates = transcription
        .segments
        .into_iter()
        .map(|segment| RecorderTranscriptUpdate::Committed {
            start_sample: segment
                .start_ms
                .saturating_mul(sample_rate)
                .saturating_div(1_000)
                .min(frame_count),
            end_sample: segment
                .end_ms
                .saturating_mul(sample_rate)
                .saturating_div(1_000)
                .min(frame_count),
            text: segment.text,
            language: transcription.language.clone(),
        })
        .collect();
    RecorderStore::open_runtime(app_support_dir)?.replace_transcript(session.id, updates)
}

#[cfg(target_os = "macos")]
fn recorder_result_error_is_terminal(kind: streaming_session::WindowResultKind) -> bool {
    kind == streaming_session::WindowResultKind::Committed
}

/// The native-rate audio writer is the Recorder's durability boundary. Live
/// ASR is a preview and must never backpressure that writer: when the decoder
/// falls behind, drop only the derived 16 kHz chunk and let the final quality
/// pass rebuild the complete transcript from the saved 48 kHz master.
#[cfg(target_os = "macos")]
fn try_forward_recorder_asr(
    tx: &SyncSender<CapturedAudioChunk>,
    chunk: CapturedAudioChunk,
) -> Result<bool> {
    match tx.try_send(chunk) {
        Ok(()) => Ok(true),
        Err(TrySendError::Full(_)) => Ok(false),
        Err(TrySendError::Disconnected(_)) => {
            Err(AppError::Capture("Recorder decoder disconnected".into()))
        }
    }
}

#[cfg(target_os = "macos")]
fn decoder_exit_failure(outcome: &std::thread::Result<()>) -> Option<String> {
    outcome.is_err().then(|| {
        "Recorder ASR runtime stopped unexpectedly; audio was preserved for recovery".into()
    })
}

#[cfg(target_os = "macos")]
pub(super) fn spawn_recorder_pipeline(
    app: tauri::AppHandle,
    app_support_dir: PathBuf,
    session: RecorderSession,
    generation: u64,
    decoder_model: RecorderDecoderModel,
    samples_rx: Receiver<PooledMicrophoneSamples>,
    mut writer: RecorderAudioWriter,
) -> std::sync::mpsc::Sender<String> {
    let timeline_start_ms = writer.duration_ms();
    let (asr_tx, asr_rx) = sync_channel(AUDIO_QUEUE_CAPACITY);
    let (results_tx, results_rx) = streaming_session::result_channel();
    let (tail_persisted_tx, tail_persisted_rx) = std::sync::mpsc::channel();
    let (audio_finalized_tx, audio_finalized_rx) = std::sync::mpsc::channel();
    let (terminal_failure_tx, terminal_failure_rx) = std::sync::mpsc::channel();
    let (decoder_finished_tx, decoder_finished_rx) = std::sync::mpsc::channel();
    let sample_rate = session.sample_rate;
    let quality_model = decoder_model.clone();

    std::thread::spawn(move || {
        let mut resampler =
            match BandlimitedChunkResampler::new(sample_rate, crate::audio::SAMPLE_RATE) {
                Ok(value) => value,
                Err(error) => {
                    let _ = audio_finalized_tx.send(Err(AppError::Audio(error.to_string())));
                    return;
                }
            };
        let mut pipeline_error = None;
        let mut dropped_asr_chunks = 0_u64;
        for samples in samples_rx {
            if let Err(error) = writer.append_f32(samples.samples()) {
                pipeline_error = Some(error);
                break;
            }
            match resampler.push_f32(samples.samples()) {
                Ok(chunk) => {
                    if !chunk.samples.is_empty() {
                        match try_forward_recorder_asr(&asr_tx, chunk) {
                            Ok(true) => {}
                            Ok(false) => {
                                dropped_asr_chunks = dropped_asr_chunks.saturating_add(1);
                                if dropped_asr_chunks == 1 {
                                    log::warn!(
                                        "Recorder live ASR queue is full; preserving audio and deferring transcript repair to the quality pass"
                                    );
                                }
                            }
                            Err(error) => {
                                pipeline_error = Some(error);
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    pipeline_error = Some(AppError::Audio(error.to_string()));
                    break;
                }
            }
        }
        if pipeline_error.is_none() {
            match resampler.finish() {
                Ok(chunk) if !chunk.samples.is_empty() => {
                    match try_forward_recorder_asr(&asr_tx, chunk) {
                        Ok(true) => {}
                        Ok(false) => {
                            dropped_asr_chunks = dropped_asr_chunks.saturating_add(1);
                        }
                        Err(error) => pipeline_error = Some(error),
                    }
                }
                Ok(_) => {}
                Err(error) => pipeline_error = Some(AppError::Audio(error.to_string())),
            }
        }
        drop(asr_tx);
        if dropped_asr_chunks > 0 {
            log::warn!(
                "Recorder deferred {dropped_asr_chunks} realtime ASR chunks to the final quality pass"
            );
        }
        let finalized = match pipeline_error {
            Some(error) => Err(error),
            None => match tail_persisted_rx.recv() {
                Ok(()) => writer.finalize(),
                Err(_) => Err(AppError::Store(
                    "Recorder transcript finalizer disconnected".into(),
                )),
            },
        };
        let _ = audio_finalized_tx.send(finalized);
    });

    std::thread::spawn(move || {
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match decoder_model {
                RecorderDecoderModel::Whisper(ctx) => streaming_session::run_windowed_decode_from(
                    move || streaming_session::WhisperSessionDecoder::new(&ctx),
                    asr_rx,
                    results_tx,
                    0,
                    timeline_start_ms,
                ),
                RecorderDecoderModel::QwenGguf(model) => {
                    streaming_session::run_windowed_decode_from(
                        move || Ok(streaming_session::QwenGgufSessionDecoder::new(model)),
                        asr_rx,
                        results_tx,
                        0,
                        timeline_start_ms,
                    )
                }
            }));
        let failure = decoder_exit_failure(&outcome);
        let _ = decoder_finished_tx.send(failure);
    });

    std::thread::spawn(move || {
        drive_recorder_results(
            app,
            app_support_dir,
            session,
            generation,
            results_rx,
            RecorderFinalizationChannels {
                tail_persisted_tx,
                audio_finalized_rx,
                decoder_finished_rx,
                terminal_failure_rx,
            },
            quality_model,
        )
    });
    terminal_failure_tx
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(super) enum RecorderDecoderModel {
    Whisper(std::sync::Arc<whisper_rs::WhisperContext>),
    QwenGguf(std::sync::Arc<crate::qwen_gguf_asr::QwenGgufAsrModel>),
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn start_recorder_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    draft_id: Option<RecorderSessionId>,
) -> Result<RecorderSessionDto> {
    start_recorder_impl(app, &state, draft_id).await
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) async fn stop_recorder_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    stop_recorder_impl(app, &state).await
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn start_recorder_session(
    _state: State<'_, AppState>,
    _draft_id: Option<RecorderSessionId>,
) -> Result<RecorderSessionDto> {
    Err(AppError::Capture(
        "Recorder microphone capture is only available on macOS".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn stop_recorder_session(_state: State<'_, AppState>) -> Result<()> {
    Err(AppError::Capture(
        "Recorder microphone capture is only available on macOS".into(),
    ))
}

#[cfg(all(test, target_os = "macos"))]
#[path = "recorder/tests.rs"]
mod tests;
