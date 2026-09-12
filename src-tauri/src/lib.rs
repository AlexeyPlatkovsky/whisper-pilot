//! WhisperPilot core: offline file transcription (language auto-detected) with
//! a summary to come.

pub mod audio;
pub mod bubble_window;
pub mod cloud_provider;
pub mod cloud_streaming;
mod commands;
pub mod diarize;
pub mod diarize_process;
pub mod error;
mod events;
pub mod live_capture;
pub mod llm;
pub mod meetings;
pub mod microphone_audio;
pub mod microphone_permission;
pub mod models;
pub mod recorder_audio;
pub mod recorder_shortcut;
pub mod recorder_store;
pub mod settings;
mod state;
pub mod store;
pub mod streaming;
pub mod streaming_audio;
pub mod streaming_session;
pub mod streaming_store;
pub mod transcribe;

pub use commands::transcription::TranscribeMeetingResult;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // This same binary is re-executed as a diarization worker (WP-53). That
    // launch must be recognized before anything else starts: it builds no
    // window and touches no Tauri state.
    if let Some(code) = diarize_process::worker_exit_code() {
        std::process::exit(code);
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let builder = tauri::Builder::default().manage(AppState::default());
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, shortcut, event| {
                commands::recorder::handle_global_shortcut(app, shortcut, event)
            })
            .build(),
    );
    builder
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                commands::recorder::setup_recorder(app)?;
                if let Err(error) = commands::bubble::setup_bubble(app) {
                    // The bubble is an optional recovery surface. A failure to
                    // create it must leave the primary application usable;
                    // collapse attempts will return a visible error instead.
                    log::error!("Recorder bubble is unavailable: {error}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::dialogs::open_file_dialog,
            commands::bubble::collapse_to_bubble,
            commands::bubble::restore_main_from_bubble,
            commands::bubble::set_bubble_always_on_top,
            commands::meetings::create_meeting,
            commands::meetings::list_meetings,
            commands::meetings::open_meeting,
            commands::meetings::rename_meeting,
            commands::meetings::delete_meeting,
            commands::meetings::update_segment,
            commands::meetings::update_mfu,
            commands::transcription::set_meeting_source,
            commands::transcription::transcribe_meeting,
            commands::transcription::diarize_meeting,
            commands::streaming::list_streaming_sessions,
            commands::streaming::open_streaming_session,
            commands::streaming::rename_streaming_session,
            commands::streaming::delete_streaming_session,
            commands::streaming::list_streaming_translations,
            commands::streaming::create_streaming_session,
            commands::streaming::start_streaming_session,
            commands::streaming::stop_streaming_session,
            commands::streaming::get_live_capture_snapshot,
            commands::recorder::get_microphone_permission_status,
            commands::recorder::request_microphone_permission,
            commands::recorder::show_recorder_workspace,
            commands::recorder::list_recorder_sessions,
            commands::recorder::open_recorder_session,
            commands::recorder::rename_recorder_session,
            commands::recorder::delete_recorder_session,
            commands::recorder::update_recorder_segment,
            commands::recorder::recover_recorder_session,
            commands::recorder::export_recorder_wav,
            commands::recorder::start_recorder_session,
            commands::recorder::stop_recorder_session,
            commands::streaming::set_streaming_translation_enabled,
            commands::dialogs::save_text_dialog,
            commands::settings::get_settings,
            commands::settings::set_setting,
            commands::settings::set_recorder_shortcut,
            commands::settings::get_recorder_shortcut_status,
            commands::settings::get_cloud_provider_config,
            commands::settings::select_cloud_provider,
            commands::settings::verify_cloud_provider_api_key,
            commands::settings::save_cloud_provider_api_key,
            commands::settings::remove_cloud_provider_api_key,
            commands::models::list_task_models,
            commands::models::download_model,
            commands::models::delete_model,
            commands::mfu::generate_mfu,
            commands::mfu::generate_streaming_mfu,
            commands::mfu::generate_streaming_prettify,
            commands::mfu::accept_streaming_prettify,
            commands::mfu::revert_streaming_prettify,
            commands::mfu::generate_recorder_polish,
            commands::mfu::accept_recorder_polish,
            commands::mfu::revert_recorder_polish,
            commands::mfu::translate_streaming_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running WhisperPilot");
}
