//! Recorder persistence, capture and live-transcript IPC.

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
use crate::state::{LiveCaptureRuntime, RecorderRuntime};
#[cfg(target_os = "macos")]
use crate::streaming_session;
#[cfg(target_os = "macos")]
use std::sync::mpsc::{sync_channel, Receiver};
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
    sample_rate: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_reason: Option<String>,
    audio_path: String,
    segments: Vec<RecorderSegment>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecorderSessionSummaryDto {
    id: i64,
    title: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    duration_ms: i64,
    status: crate::recorder_store::RecorderStatus,
    sample_rate: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_reason: Option<String>,
    audio_path: String,
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
            sample_rate: session.sample_rate,
            recovery_reason: session.recovery_reason,
            audio_path: session.audio_path.to_string_lossy().into_owned(),
        }
    }
}

fn session_dto(store: &RecorderStore, session: RecorderSession) -> Result<RecorderSessionDto> {
    let segments = store.list_segments(session.id)?;
    Ok(RecorderSessionDto {
        id: session.id,
        title: session.title,
        created_at_ms: session.created_at_ms,
        updated_at_ms: session.updated_at_ms,
        duration_ms: session.duration_ms,
        status: session.status,
        sample_rate: session.sample_rate,
        recovery_reason: session.recovery_reason,
        audio_path: session.audio_path.to_string_lossy().into_owned(),
        segments,
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
                if let Err(error) = start_recorder_impl(app.clone(), &app.state::<AppState>()).await
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
                if result.kind == streaming_session::WindowResultKind::Gap {
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

    if let Ok(message) = finalization.terminal_failure_rx.try_recv() {
        terminal_error = Some(message);
    }

    let _ = finalization.tail_persisted_tx.send(());
    if let Err(error) = finalization
        .audio_finalized_rx
        .recv()
        .unwrap_or_else(|_| Err(AppError::Audio("Recorder audio writer disconnected".into())))
    {
        terminal_error = Some(format!("Recorder audio could not be finalized: {error}"));
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
fn spawn_recorder_pipeline(
    app: tauri::AppHandle,
    app_support_dir: PathBuf,
    session: RecorderSession,
    generation: u64,
    ctx: std::sync::Arc<whisper_rs::WhisperContext>,
    samples_rx: Receiver<PooledMicrophoneSamples>,
    mut writer: RecorderAudioWriter,
) -> std::sync::mpsc::Sender<String> {
    let (asr_tx, asr_rx) = sync_channel(AUDIO_QUEUE_CAPACITY);
    let (results_tx, results_rx) = streaming_session::result_channel();
    let (tail_persisted_tx, tail_persisted_rx) = std::sync::mpsc::channel();
    let (audio_finalized_tx, audio_finalized_rx) = std::sync::mpsc::channel();
    let (terminal_failure_tx, terminal_failure_rx) = std::sync::mpsc::channel();
    let sample_rate = session.sample_rate;

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
        for samples in samples_rx {
            if let Err(error) = writer.append_f32(samples.samples()) {
                pipeline_error = Some(error);
                break;
            }
            match resampler.push_f32(samples.samples()) {
                Ok(chunk) => {
                    if !chunk.samples.is_empty() && asr_tx.send(chunk).is_err() {
                        pipeline_error =
                            Some(AppError::Capture("Recorder decoder disconnected".into()));
                        break;
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
                    if asr_tx.send(chunk).is_err() {
                        pipeline_error =
                            Some(AppError::Capture("Recorder decoder disconnected".into()));
                    }
                }
                Ok(_) => {}
                Err(error) => pipeline_error = Some(AppError::Audio(error.to_string())),
            }
        }
        drop(asr_tx);
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
        streaming_session::run_windowed_decode(
            move || streaming_session::WhisperSessionDecoder::new(&ctx),
            asr_rx,
            results_tx,
            0,
        )
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
                terminal_failure_rx,
            },
        )
    });
    terminal_failure_tx
}

#[cfg(target_os = "macos")]
pub(crate) async fn start_recorder_impl(
    app: tauri::AppHandle,
    state: &AppState,
) -> Result<RecorderSessionDto> {
    let app_support_dir = app_data_dir(&app)?;
    let info = MicrophoneCaptureSession::probe_default_input()?;
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
    let ctx = match state.model(app_support_dir.clone()).await {
        Ok(ctx) => ctx,
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
    let store = match RecorderStore::open_runtime(&app_support_dir) {
        Ok(store) => store,
        Err(error) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    };
    let session = match store.create_session(NewRecorderSession {
        title: format!("Recording {now}"),
        created_at_ms: now,
        sample_rate: info.sample_rate,
    }) {
        Ok(session) => session,
        Err(error) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    };
    let writer = match RecorderAudioWriter::create(&session.audio_path, session.sample_rate) {
        Ok(writer) => writer,
        Err(error) => {
            let _ = store.discard_failed_start(session.id);
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    };
    let generation = match begin_recorder_capture(&app, state, session.id) {
        Ok(value) => value,
        Err(error) => {
            drop(writer);
            let _ = store.discard_failed_start(session.id);
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(error);
        }
    };
    let (samples_tx, samples_rx) = sync_channel(AUDIO_QUEUE_CAPACITY);
    let started = match MicrophoneCaptureSession::start(samples_tx) {
        Ok(started) => started,
        Err(error) => {
            drop(writer);
            let _ = store.discard_failed_start(session.id);
            streaming_session::release_whisper_busy(&state.whisper_busy);
            fail_recorder_capture(&app, generation, &error.to_string());
            return Err(error);
        }
    };
    if started.info.sample_rate != session.sample_rate {
        drop(started);
        drop(writer);
        let _ = store.discard_failed_start(session.id);
        streaming_session::release_whisper_busy(&state.whisper_busy);
        let error = AppError::Capture(
            "default microphone format changed during Recorder startup; retry".into(),
        );
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
            let _ = store.discard_failed_start(session.id);
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(AppError::Capture(
                "live capture coordinator lock is poisoned".into(),
            ));
        }
    };
    let snapshot = match installation {
        Ok(snapshot) => snapshot,
        Err(runtime) => {
            drop(runtime);
            drop(writer);
            let _ = store.discard_failed_start(session.id);
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(AppError::Capture("Recorder start was cancelled".into()));
        }
    };
    let _ = app.emit("live_capture_state", snapshot);
    let terminal_failure_tx = spawn_recorder_pipeline(
        app.clone(),
        app_support_dir.clone(),
        session.clone(),
        generation,
        ctx,
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
) -> Result<RecorderSessionDto> {
    start_recorder_impl(app, &state).await
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
