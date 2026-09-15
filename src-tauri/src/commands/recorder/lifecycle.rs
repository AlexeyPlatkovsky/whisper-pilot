//! Recorder capture lifecycle kept separate from IPC registration and pipeline workers.

use super::{
    begin_recorder_capture, emit_error, emit_session, fail_recorder_capture,
    spawn_recorder_pipeline, RecorderDecoderModel, AUDIO_QUEUE_CAPACITY,
};
use crate::asr::{self, AsrLanguage, AsrMode, AsrRuntime};
use crate::error::{AppError, Result};
use crate::live_capture::{LiveCapturePhase, LiveCaptureSource};
use crate::microphone_audio::MicrophoneCaptureSession;
use crate::recorder_audio::RecorderAudioWriter;
use crate::recorder_store::{NewRecorderSession, RecorderSessionId, RecorderStore};
use crate::state::{app_data_dir, now_ms, AppState, LiveCaptureRuntime, RecorderRuntime};
use crate::streaming_session;
use std::sync::mpsc::sync_channel;
use tauri::Emitter;

pub(crate) async fn start_recorder_impl(
    app: tauri::AppHandle,
    state: &AppState,
    draft_id: Option<RecorderSessionId>,
) -> Result<super::RecorderSessionDto> {
    let app_support_dir = app_data_dir(&app)?;
    let _asr_mutation = state.recorder_asr_mutation.lock().await;
    let store = RecorderStore::open_runtime(&app_support_dir)?;
    let requested = draft_id
        .map(|id| {
            store
                .get_session(id)?
                .filter(|session| {
                    session.is_draft
                        || session.status == crate::recorder_store::RecorderStatus::Completed
                })
                .ok_or_else(|| {
                    AppError::Store(format!(
                        "Recorder session {id} is not ready to start or continue"
                    ))
                })
        })
        .transpose()?;
    let continuing = requested.as_ref().is_some_and(|session| !session.is_draft);
    let recorder_settings = crate::settings::get_settings(&app_support_dir);
    let language = AsrLanguage::Auto;
    let asr_spec = asr::resolve_selection(
        requested
            .as_ref()
            .map(|session| session.asr_model_id.as_str())
            .or(recorder_settings.active_model_transcription.as_deref())
            .unwrap_or(asr::DEFAULT_ASR_MODEL_ID),
        AsrMode::Recorder,
        language,
    )?;
    let info = MicrophoneCaptureSession::probe_default_input()?;
    if let Some(existing) = requested.as_ref().filter(|_| continuing) {
        if info.sample_rate != existing.sample_rate {
            return Err(AppError::Capture(format!(
                "the selected microphone uses {} Hz, but this recording uses {} Hz; select the original input device or create a new recording",
                info.sample_rate, existing.sample_rate
            )));
        }
    }
    let prior_polished = requested
        .as_ref()
        .filter(|_| continuing)
        .map(|session| store.get_polished(session.id))
        .transpose()?
        .flatten();
    if let Err(holder) = streaming_session::try_claim_recorder(&state.whisper_busy) {
        return Err(AppError::Capture(match holder {
            streaming_session::WhisperUser::Meeting => {
                "a file transcription is currently running; wait before starting Recorder".into()
            }
            streaming_session::WhisperUser::Streaming => {
                "Meeting is currently capturing; stop it before starting Recorder".into()
            }
            streaming_session::WhisperUser::Recorder => "Recorder is already running".into(),
        }));
    }
    let decoder_model = match asr_spec.runtime {
        AsrRuntime::WhisperCpp => state
            .model(app_support_dir.clone())
            .await
            .map(RecorderDecoderModel::Whisper),
        AsrRuntime::LlamaCppMtmd => state
            .qwen_gguf_asr_model(app_support_dir.clone(), asr_spec)
            .await
            .map(RecorderDecoderModel::QwenGguf),
    };
    let decoder_model = match decoder_model {
        Ok(model) => model,
        Err(error) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    };
    let now = match now_ms() {
        Ok(value) => value,
        Err(error) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    };
    let session = match requested {
        Some(session) if session.is_draft => store.activate_draft(session.id, info.sample_rate),
        Some(session) => store.resume_session(session.id),
        None => store.create_session(NewRecorderSession {
            title: format!("Recording {now}"),
            created_at_ms: now,
            sample_rate: info.sample_rate,
            asr_model_id: asr_spec.model_id.to_string(),
            asr_engine: asr_spec.engine.as_str().to_string(),
            asr_language: language.code().to_string(),
        }),
    };
    let session = match session {
        Ok(session) => session,
        Err(error) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    };
    let rollback_failed_start = |store: &RecorderStore, id, error: AppError| {
        let rollback = if continuing {
            store.restore_completed_after_failed_resume(id)
        } else if draft_id.is_some() {
            store.restore_draft_after_failed_start(id)
        } else {
            store.discard_failed_start(id)
        };
        let rollback = rollback.and_then(|()| {
            if continuing {
                if let Some(text) = prior_polished.as_deref() {
                    store.upsert_polished(id, text)?;
                }
            }
            Ok(())
        });
        match rollback {
            Ok(()) => error,
            Err(rollback) => AppError::Capture(format!(
                "{error}; Recorder startup rollback also failed: {rollback}"
            )),
        }
    };
    let writer_path = session.audio_path.clone();
    let writer_rate = session.sample_rate;
    let writer_result = tokio::task::spawn_blocking(move || {
        if continuing {
            RecorderAudioWriter::resume(&writer_path, writer_rate)
        } else {
            RecorderAudioWriter::create(&writer_path, writer_rate)
        }
    })
    .await
    .map_err(|error| AppError::Audio(format!("Recorder audio setup task failed: {error}")))
    .and_then(|result| result);
    let writer = match writer_result {
        Ok(writer) => writer,
        Err(error) => {
            let error = rollback_failed_start(&store, session.id, error);
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    };
    if continuing {
        if let Err(error) = store.delete_polished(session.id) {
            drop(writer);
            let error = rollback_failed_start(&store, session.id, error);
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    }
    let generation = match begin_recorder_capture(&app, state, session.id) {
        Ok(value) => value,
        Err(error) => {
            drop(writer);
            let error = rollback_failed_start(&store, session.id, error);
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    };
    let (samples_tx, samples_rx) = sync_channel(AUDIO_QUEUE_CAPACITY);
    let started = match MicrophoneCaptureSession::start(samples_tx) {
        Ok(started) => started,
        Err(error) => {
            drop(writer);
            let error = rollback_failed_start(&store, session.id, error);
            streaming_session::release_whisper_busy(&state.whisper_busy);
            fail_recorder_capture(&app, generation, &error.to_string());
            return Err(error);
        }
    };
    if started.info.sample_rate != session.sample_rate {
        drop(started);
        drop(writer);
        let error = AppError::Capture(
            "default microphone format changed during Recorder startup; retry".into(),
        );
        let error = rollback_failed_start(&store, session.id, error);
        streaming_session::release_whisper_busy(&state.whisper_busy);
        fail_recorder_capture(&app, generation, &error.to_string());
        return Err(error);
    }
    let runtime = LiveCaptureRuntime::Recorder(RecorderRuntime {
        session_id: session.id,
        capture: started.session,
    });
    let installation = match state.live_capture.lock() {
        Ok(mut coordinator) => coordinator.install_runtime(generation, runtime),
        Err(_) => {
            drop(writer);
            let error = rollback_failed_start(
                &store,
                session.id,
                AppError::Capture("live capture coordinator lock is poisoned".into()),
            );
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    };
    let snapshot = match installation {
        Ok(snapshot) => snapshot,
        Err(runtime) => {
            drop(runtime);
            drop(writer);
            let error = rollback_failed_start(
                &store,
                session.id,
                AppError::Capture("Recorder start was cancelled".into()),
            );
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    };
    let _ = app.emit("live_capture_state", snapshot);
    let terminal_failure_tx = spawn_recorder_pipeline(
        app.clone(),
        app_support_dir.clone(),
        session.clone(),
        generation,
        decoder_model,
        samples_rx,
        writer,
    );
    let monitor_app = app.clone();
    std::thread::spawn(move || {
        if let Ok(failure) = started.failures.recv() {
            let _ = terminal_failure_tx.send(failure.message.clone());
            emit_error(&monitor_app, Some(session.id), &failure.message);
            fail_recorder_capture(&monitor_app, generation, &failure.message);
        }
    });
    emit_session(&app, &app_support_dir, session.id);
    super::open_dto(&app_support_dir, session.id)
}

pub(crate) async fn stop_recorder_impl(app: tauri::AppHandle, state: &AppState) -> Result<()> {
    let snapshot = state
        .live_capture
        .lock()
        .map_err(|_| AppError::Capture("live capture coordinator lock is poisoned".into()))?
        .snapshot();
    if snapshot.source != Some(LiveCaptureSource::Recorder)
        || !matches!(
            snapshot.phase,
            LiveCapturePhase::Starting | LiveCapturePhase::Capturing
        )
    {
        return Err(AppError::Capture(
            "Recorder is not currently capturing".into(),
        ));
    }
    let id = snapshot
        .session_id
        .ok_or_else(|| AppError::Capture("Recorder has no active session".into()))?;
    let app_support_dir = app_data_dir(&app)?;
    RecorderStore::open_runtime(&app_support_dir)?.mark_finalizing(id)?;
    emit_session(&app, &app_support_dir, id);
    let (stopping, runtime) = state
        .live_capture
        .lock()
        .map_err(|_| AppError::Capture("live capture coordinator lock is poisoned".into()))?
        .begin_stop_for(LiveCaptureSource::Recorder)
        .map_err(|error| AppError::Capture(error.to_string()))?;
    let _ = app.emit("live_capture_state", stopping);
    drop(runtime);
    Ok(())
}
