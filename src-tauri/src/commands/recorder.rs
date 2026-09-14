//! Recorder persistence, capture and live-transcript IPC.

use crate::asr::{self, AsrLanguage, AsrMode, AsrRuntime};
use crate::error::{AppError, Result};
use crate::microphone_permission::{self, MicrophonePermissionStatus};
use crate::recorder_audio::{export_caf_to_wav, RecorderAudioWriter};
use crate::recorder_store::{
    NewRecorderSession, RecorderSegment, RecorderSession, RecorderSessionId, RecorderStore,
    RecorderTranscriptUpdate,
};
use crate::state::{app_data_dir, now_ms, AppState};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

#[cfg(target_os = "macos")]
use crate::events::{RecorderErrorEvent, RecorderPartialEvent};
#[cfg(target_os = "macos")]
use crate::live_capture::{LiveCapturePhase, LiveCaptureSource};
#[cfg(target_os = "macos")]
use crate::microphone_audio::{
    BandlimitedChunkResampler, MicrophoneCaptureSession, PooledMicrophoneSamples,
};
#[cfg(target_os = "macos")]
use crate::recorder_audio::read_caf_audio;
#[cfg(target_os = "macos")]
use crate::state::{LiveCaptureRuntime, RecorderRuntime};
#[cfg(target_os = "macos")]
use crate::streaming_audio::CapturedAudioChunk;
#[cfg(target_os = "macos")]
use crate::streaming_session;
#[cfg(target_os = "macos")]
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
#[cfg(target_os = "macos")]
use tauri::{Emitter, Manager};
#[cfg(target_os = "macos")]
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

#[cfg(target_os = "macos")]
const AUDIO_QUEUE_CAPACITY: usize = 32;
#[cfg(target_os = "macos")]
const CAPTION_WINDOW_LABEL: &str = "recorder-caption";

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecorderSessionDto {
    id: i64,
    title: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    duration_ms: i64,
    status: crate::recorder_store::RecorderStatus,
    is_draft: bool,
    sample_rate: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_reason: Option<String>,
    audio_path: String,
    asr_model_id: String,
    asr_engine: String,
    asr_language: String,
    segments: Vec<RecorderSegment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    polished_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecorderSessionSummaryDto {
    id: i64,
    title: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    duration_ms: i64,
    status: crate::recorder_store::RecorderStatus,
    is_draft: bool,
    sample_rate: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_reason: Option<String>,
    audio_path: String,
    asr_model_id: String,
    asr_engine: String,
    asr_language: String,
}

impl From<RecorderSession> for RecorderSessionSummaryDto {
    fn from(session: RecorderSession) -> Self {
        Self {
            id: session.id,
            title: session.title,
            created_at_ms: session.created_at_ms,
            updated_at_ms: session.updated_at_ms,
            duration_ms: session.duration_ms,
            status: session.status,
            is_draft: session.is_draft,
            sample_rate: session.sample_rate,
            recovery_reason: session.recovery_reason,
            audio_path: session.audio_path.to_string_lossy().into_owned(),
            asr_model_id: session.asr_model_id,
            asr_engine: session.asr_engine,
            asr_language: session.asr_language,
        }
    }
}

fn session_dto(store: &RecorderStore, session: RecorderSession) -> Result<RecorderSessionDto> {
    let segments = store.list_segments(session.id)?;
    let polished_text = store.get_polished(session.id)?;
    Ok(RecorderSessionDto {
        id: session.id,
        title: session.title,
        created_at_ms: session.created_at_ms,
        updated_at_ms: session.updated_at_ms,
        duration_ms: session.duration_ms,
        status: session.status,
        is_draft: session.is_draft,
        sample_rate: session.sample_rate,
        recovery_reason: session.recovery_reason,
        audio_path: session.audio_path.to_string_lossy().into_owned(),
        asr_model_id: session.asr_model_id,
        asr_engine: session.asr_engine,
        asr_language: session.asr_language,
        segments,
        polished_text,
    })
}

fn open_dto(app_support_dir: &Path, id: RecorderSessionId) -> Result<RecorderSessionDto> {
    let store = RecorderStore::open_runtime(app_support_dir)?;
    let session = store
        .get_session(id)?
        .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))?;
    session_dto(&store, session)
}

#[tauri::command]
pub(crate) fn get_microphone_permission_status() -> MicrophonePermissionStatus {
    microphone_permission::get_microphone_permission_status()
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) fn show_recorder_workspace(app: tauri::AppHandle) -> Result<()> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::Capture("WhisperPilot main window is unavailable".into()))?;
    main.show()
        .map_err(|error| AppError::Io(error.to_string()))?;
    main.unminimize()
        .map_err(|error| AppError::Io(error.to_string()))?;
    app.emit_to("main", "open_recorder_workspace", ())
        .map_err(|error| AppError::Io(error.to_string()))?;
    main.set_focus()
        .map_err(|error| AppError::Io(error.to_string()))
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) fn show_recorder_workspace(_app: tauri::AppHandle) -> Result<()> {
    Err(AppError::Capture(
        "Recorder workspace is only available on macOS".into(),
    ))
}

#[tauri::command]
pub(crate) async fn request_microphone_permission() -> MicrophonePermissionStatus {
    microphone_permission::request_microphone_permission().await
}

#[tauri::command]
pub(crate) fn list_recorder_sessions(
    app: tauri::AppHandle,
) -> Result<Vec<RecorderSessionSummaryDto>> {
    let app_support_dir = app_data_dir(&app)?;
    let store = RecorderStore::open_runtime(&app_support_dir)?;
    Ok(store
        .list_sessions()?
        .into_iter()
        .map(RecorderSessionSummaryDto::from)
        .collect())
}

#[tauri::command]
pub(crate) fn open_recorder_session(
    app: tauri::AppHandle,
    id: RecorderSessionId,
) -> Result<RecorderSessionDto> {
    open_dto(&app_data_dir(&app)?, id)
}

#[tauri::command]
pub(crate) fn create_recorder_draft(app: tauri::AppHandle) -> Result<RecorderSessionDto> {
    let app_support_dir = app_data_dir(&app)?;
    let settings = crate::settings::get_settings(&app_support_dir);
    let language = AsrLanguage::Auto;
    let asr_spec = asr::resolve_selection(
        settings
            .active_model_transcription
            .as_deref()
            .unwrap_or(asr::DEFAULT_ASR_MODEL_ID),
        AsrMode::Recorder,
        language,
    )?;
    let now = now_ms()?;
    let session =
        RecorderStore::open_runtime(&app_support_dir)?.create_draft(NewRecorderSession {
            title: "Untitled recording".into(),
            created_at_ms: now,
            sample_rate: 48_000,
            asr_model_id: asr_spec.model_id.to_string(),
            asr_engine: asr_spec.engine.as_str().to_string(),
            asr_language: language.code().to_string(),
        })?;
    open_dto(&app_support_dir, session.id)
}

#[tauri::command]
pub(crate) fn rename_recorder_session(
    app: tauri::AppHandle,
    id: RecorderSessionId,
    title: String,
) -> Result<RecorderSessionDto> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::Store("Recorder title must not be empty".into()));
    }
    let app_support_dir = app_data_dir(&app)?;
    RecorderStore::open_runtime(&app_support_dir)?.rename_session(id, title)?;
    open_dto(&app_support_dir, id)
}

#[tauri::command]
pub(crate) fn delete_recorder_session(app: tauri::AppHandle, id: RecorderSessionId) -> Result<()> {
    ensure_recorder_session_is_not_live(&app, id)?;
    RecorderStore::open_runtime(&app_data_dir(&app)?)?.delete_session(id)
}

#[tauri::command]
pub(crate) fn clear_recorder_recording(
    app: tauri::AppHandle,
    id: RecorderSessionId,
) -> Result<RecorderSessionDto> {
    ensure_recorder_session_is_not_live(&app, id)?;
    let app_support_dir = app_data_dir(&app)?;
    RecorderStore::open_runtime(&app_support_dir)?.clear_recording(id)?;
    open_dto(&app_support_dir, id)
}

#[tauri::command]
pub(crate) fn update_recorder_segment(
    app: tauri::AppHandle,
    session_id: RecorderSessionId,
    segment_id: i64,
    text: String,
) -> Result<RecorderSegment> {
    let store = RecorderStore::open_runtime(&app_data_dir(&app)?)?;
    store.update_segment_text(session_id, segment_id, &text)?;
    store
        .get_segment(session_id, segment_id)?
        .ok_or_else(|| AppError::Store(format!("Recorder segment {segment_id} was not found")))
}

#[tauri::command]
pub(crate) fn recover_recorder_session(
    app: tauri::AppHandle,
    id: RecorderSessionId,
) -> Result<RecorderSessionDto> {
    ensure_recorder_session_is_not_live(&app, id)?;
    let app_support_dir = app_data_dir(&app)?;
    let store = RecorderStore::open_runtime(&app_support_dir)?;
    store.recover_session(id)?;
    open_dto(&app_support_dir, id)
}

fn ensure_recorder_session_is_not_live(app: &tauri::AppHandle, id: i64) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
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

#[tauri::command]
pub(crate) async fn export_recorder_wav(
    app: tauri::AppHandle,
    id: RecorderSessionId,
) -> Result<Option<String>> {
    let app_support_dir = app_data_dir(&app)?;
    let session = RecorderStore::open_runtime(&app_support_dir)?
        .get_session(id)?
        .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))?;
    let Some(path) = rfd::AsyncFileDialog::new()
        .add_filter("Wave audio", &["wav"])
        .set_file_name(format!("{}.wav", safe_file_stem(&session.title)))
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let target = path.path().to_path_buf();
    export_caf_to_wav(&session.audio_path, &target)?;
    Ok(Some(target.to_string_lossy().into_owned()))
}

fn safe_file_stem(title: &str) -> String {
    let safe = title
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' => '-',
            other => other,
        })
        .collect::<String>();
    let trimmed = safe.trim();
    if trimmed.is_empty() {
        "recording".into()
    } else {
        trimmed.into()
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn setup_recorder(
    app: &mut tauri::App,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let app_support_dir = app_data_dir(app.handle())?;
    // This is the one launch-time reconciliation pass. Live commands use
    // `open_runtime` so an active `.partial` is never mistaken for a crash.
    let _ = RecorderStore::open(&app_support_dir)?;
    if app.get_webview_window(CAPTION_WINDOW_LABEL).is_none() {
        let caption = tauri::WebviewWindowBuilder::new(
            app,
            CAPTION_WINDOW_LABEL,
            tauri::WebviewUrl::App("index.html?window=recorder-caption".into()),
        )
        .title("WhisperPilot Recorder")
        .inner_size(520.0, 140.0)
        .min_inner_size(360.0, 120.0)
        .decorations(false)
        .resizable(true)
        .always_on_top(true)
        .focused(false)
        .visible(false)
        .skip_taskbar(true)
        .build()?;
        let caption_app = app.handle().clone();
        caption.on_window_event(move |event| {
            if !matches!(event, tauri::WindowEvent::Destroyed) {
                return;
            }
            let should_finalize = caption_app
                .state::<AppState>()
                .live_capture
                .lock()
                .ok()
                .map(|coordinator| coordinator.snapshot())
                .is_some_and(|snapshot| {
                    snapshot.source == Some(LiveCaptureSource::Recorder)
                        && matches!(
                            snapshot.phase,
                            LiveCapturePhase::Starting | LiveCapturePhase::Capturing
                        )
                });
            if should_finalize {
                let app = caption_app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = stop_recorder_impl(app.clone(), &app.state::<AppState>()).await;
                });
            }
        });
    }
    let configured = crate::settings::get_settings(&app_support_dir).recorder_shortcut;
    match crate::commands::settings::platform_shortcut(&configured) {
        Ok(shortcut) => match app.global_shortcut().register(shortcut) {
            Ok(()) => {
                if let Ok(mut current) = app.state::<AppState>().registered_recorder_shortcut.lock()
                {
                    *current = Some(shortcut);
                }
            }
            Err(error) => {
                let message =
                    format!("Recorder shortcut is disabled because registration failed: {error}");
                if let Ok(mut current) = app.state::<AppState>().recorder_shortcut_error.lock() {
                    *current = Some(message.clone());
                }
                log::warn!("{message}")
            }
        },
        Err(error) => {
            let message = format!("Recorder shortcut is disabled: {error}");
            if let Ok(mut current) = app.state::<AppState>().recorder_shortcut_error.lock() {
                *current = Some(message.clone());
            }
            log::warn!("{message}");
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn handle_global_shortcut(
    app: &tauri::AppHandle,
    shortcut: &Shortcut,
    event: ShortcutEvent,
) {
    let state = app.state::<AppState>();
    let is_current = state
        .registered_recorder_shortcut
        .lock()
        .ok()
        .and_then(|current| current.as_ref().copied())
        .is_some_and(|current| &current == shortcut);
    if !is_current {
        return;
    }
    let snapshot = match state.live_capture.lock() {
        Ok(coordinator) => coordinator.snapshot(),
        Err(_) => return,
    };
    let shortcut_state = match snapshot.phase {
        LiveCapturePhase::Idle => crate::recorder_shortcut::RecorderShortcutCaptureState::Idle,
        LiveCapturePhase::Starting => {
            crate::recorder_shortcut::RecorderShortcutCaptureState::Starting
        }
        LiveCapturePhase::Capturing => {
            crate::recorder_shortcut::RecorderShortcutCaptureState::Capturing
        }
        LiveCapturePhase::Stopping => {
            crate::recorder_shortcut::RecorderShortcutCaptureState::Stopping
        }
        LiveCapturePhase::Error => crate::recorder_shortcut::RecorderShortcutCaptureState::Error,
    };
    let shortcut_event = match event.state() {
        ShortcutState::Pressed => {
            crate::recorder_shortcut::RecorderShortcutEvent::Pressed { repeat: false }
        }
        ShortcutState::Released => crate::recorder_shortcut::RecorderShortcutEvent::Released,
    };
    let action = match state.recorder_shortcut_gate.lock() {
        Ok(mut gate) => gate.handle(
            shortcut_event,
            crate::recorder_shortcut::RecorderShortcutContext {
                state: shortcut_state,
                owner: snapshot.source,
            },
        ),
        Err(_) => return,
    };
    let app = app.clone();
    match action {
        crate::recorder_shortcut::RecorderShortcutAction::StartRecorder => {
            tauri::async_runtime::spawn(async move {
                if let Err(error) =
                    start_recorder_impl(app.clone(), &app.state::<AppState>(), None).await
                {
                    emit_error(&app, None, error.to_string());
                }
                if let Some(window) = app.get_webview_window(CAPTION_WINDOW_LABEL) {
                    let _ = window.show();
                }
            });
        }
        crate::recorder_shortcut::RecorderShortcutAction::StopRecorder => {
            tauri::async_runtime::spawn(async move {
                if let Err(error) = stop_recorder_impl(app.clone(), &app.state::<AppState>()).await
                {
                    emit_error(&app, snapshot.session_id, error.to_string());
                }
            });
        }
        crate::recorder_shortcut::RecorderShortcutAction::RejectOtherLiveSource => {
            emit_error(
                &app,
                None,
                "Another live capture is active; stop it before starting Recorder",
            );
        }
        crate::recorder_shortcut::RecorderShortcutAction::Ignore => {}
    }
}

#[cfg(target_os = "macos")]
fn emit_session(app: &tauri::AppHandle, app_support_dir: &Path, id: i64) {
    match open_dto(app_support_dir, id) {
        Ok(session) => {
            let _ = app.emit("recorder_session_changed", session);
        }
        Err(error) => log::error!("Recorder {id}: could not emit session state: {error}"),
    }
}

#[cfg(target_os = "macos")]
fn emit_error(app: &tauri::AppHandle, session_id: Option<i64>, message: impl Into<String>) {
    let _ = app.emit(
        "recorder_error",
        RecorderErrorEvent {
            session_id,
            message: message.into(),
        },
    );
}

#[cfg(target_os = "macos")]
fn begin_recorder_capture(app: &tauri::AppHandle, state: &AppState, id: i64) -> Result<u64> {
    let snapshot = state
        .live_capture
        .lock()
        .map_err(|_| AppError::Capture("live capture coordinator lock is poisoned".into()))?
        .begin_start(id, LiveCaptureSource::Recorder)
        .map_err(|error| AppError::Capture(error.to_string()))?;
    let generation = snapshot.generation;
    let _ = app.emit("live_capture_state", snapshot);
    Ok(generation)
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
fn fail_recorder_capture(app: &tauri::AppHandle, generation: u64, message: &str) {
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
    results_rx: Receiver<streaming_session::WindowResult>,
    finalization: RecorderFinalizationChannels,
    quality_model: RecorderDecoderModel,
) {
    let mut terminal_error: Option<String> = None;
    let mut partial_revision = 0_u64;
    for result in results_rx {
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
        match RecorderStore::open_runtime(&app_support_dir)
            .and_then(|store| store.apply_transcript_update(session.id, update))
        {
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
    match (
        RecorderStore::open_runtime(&app_support_dir),
        terminal_error,
    ) {
        (Ok(store), None) => {
            if let Err(error) = store.mark_completed(session.id) {
                let message = format!("Recorder final status could not be saved: {error}");
                let _ = store.mark_recoverable(session.id, &message);
                emit_error(&app, Some(session.id), message);
            }
        }
        (Ok(store), Some(message)) => {
            let _ = store.mark_recoverable(session.id, &message);
            emit_error(&app, Some(session.id), message);
        }
        (Err(error), _) => emit_error(
            &app,
            Some(session.id),
            format!("Recorder storage could not be reopened: {error}"),
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
fn spawn_recorder_pipeline(
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
enum RecorderDecoderModel {
    Whisper(std::sync::Arc<whisper_rs::WhisperContext>),
    QwenGguf(std::sync::Arc<crate::qwen_gguf_asr::QwenGgufAsrModel>),
}

#[cfg(target_os = "macos")]
pub(crate) async fn start_recorder_impl(
    app: tauri::AppHandle,
    state: &AppState,
    draft_id: Option<RecorderSessionId>,
) -> Result<RecorderSessionDto> {
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
                "a meeting is currently transcribing; wait before starting Recorder".into()
            }
            streaming_session::WhisperUser::Streaming => {
                "Streaming is currently capturing; stop it before starting Recorder".into()
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
    open_dto(&app_support_dir, session.id)
}

#[cfg(target_os = "macos")]
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
mod tests {
    use super::{
        decoder_exit_failure, recorder_result_error_is_terminal, try_forward_recorder_asr,
        RecorderSessionSummaryDto,
    };
    use crate::recorder_store::RecorderStatus;
    use crate::streaming_audio::CapturedAudioChunk;
    use crate::streaming_session::WindowResultKind;
    use std::sync::mpsc::sync_channel;

    #[test]
    fn recorder_summary_serializes_draft_state_for_the_renderer() {
        let summary = RecorderSessionSummaryDto {
            id: 7,
            title: "Draft".into(),
            created_at_ms: 10,
            updated_at_ms: 10,
            duration_ms: 0,
            status: RecorderStatus::Completed,
            is_draft: true,
            sample_rate: 48_000,
            recovery_reason: None,
            audio_path: "/tmp/7.caf".into(),
            asr_model_id: "transcription".into(),
            asr_engine: "whisper".into(),
            asr_language: "auto".into(),
        };

        let json = serde_json::to_value(summary).unwrap();

        assert_eq!(json["is_draft"], true);
        assert_eq!(json["status"], "completed");
    }

    #[test]
    fn only_committed_decode_errors_make_recorder_recoverable() {
        assert!(recorder_result_error_is_terminal(
            WindowResultKind::Committed
        ));
        assert!(!recorder_result_error_is_terminal(WindowResultKind::Gap));
        assert!(!recorder_result_error_is_terminal(
            WindowResultKind::Partial
        ));
    }

    #[test]
    fn full_realtime_asr_queue_never_blocks_or_fails_audio_persistence() {
        let (tx, _rx) = sync_channel(1);
        tx.send(CapturedAudioChunk {
            start_sample: 0,
            captured_end_sample: 1,
            samples: vec![0.1],
        })
        .unwrap();

        let forwarded = try_forward_recorder_asr(
            &tx,
            CapturedAudioChunk {
                start_sample: 1,
                captured_end_sample: 2,
                samples: vec![0.2],
            },
        )
        .expect("a full live-ASR queue is a recoverable preview drop");

        assert!(!forwarded, "the overloaded realtime chunk is dropped");
    }

    #[test]
    fn decoder_panic_becomes_a_synchronized_recoverable_failure() {
        let outcome = std::panic::catch_unwind(|| panic!("decoder failure"));

        let message = decoder_exit_failure(&outcome).expect("panic must be terminal");

        assert!(message.contains("audio was preserved for recovery"));
    }
}
