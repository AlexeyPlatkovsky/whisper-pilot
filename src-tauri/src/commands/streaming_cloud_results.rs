//! Cloud Meeting result consumption and persistence.

use super::*;

pub(super) fn drive_cloud_results(
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
                    let message = "Meeting stopped because a transcript window could not be saved."
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
                        "Meeting stopped because an overload gap could not be saved.".to_string();
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
