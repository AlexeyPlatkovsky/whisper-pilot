//! Recorder startup, hotkey, and live-capture event integration.

use super::{open_dto, start_recorder_impl, stop_recorder_impl, CAPTION_WINDOW_LABEL};
use crate::error::{AppError, Result};
use crate::events::RecorderErrorEvent;
use crate::live_capture::{LiveCapturePhase, LiveCaptureSource};
use crate::recorder_store::RecorderStore;
use crate::state::{app_data_dir, AppState};
use std::path::Path;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

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
pub(crate) fn emit_session(app: &tauri::AppHandle, app_support_dir: &Path, id: i64) {
    match open_dto(app_support_dir, id) {
        Ok(session) => {
            let _ = app.emit("recorder_session_changed", session);
        }
        Err(error) => log::error!("Recorder {id}: could not emit session state: {error}"),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn emit_error(
    app: &tauri::AppHandle,
    session_id: Option<i64>,
    message: impl Into<String>,
) {
    let _ = app.emit(
        "recorder_error",
        RecorderErrorEvent {
            session_id,
            message: message.into(),
        },
    );
}

#[cfg(target_os = "macos")]
pub(crate) fn begin_recorder_capture(
    app: &tauri::AppHandle,
    state: &AppState,
    id: i64,
) -> Result<u64> {
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
