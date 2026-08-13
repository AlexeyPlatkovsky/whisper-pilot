//! WhisperPilot core: offline file transcription (language auto-detected) with
//! a summary to come.

pub mod audio;
mod commands;
pub mod diarize;
pub mod diarize_process;
pub mod error;
mod events;
pub mod llm;
pub mod meetings;
pub mod models;
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

    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::dialogs::open_file_dialog,
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
            commands::streaming::create_streaming_session,
            commands::streaming::start_streaming_session,
            commands::streaming::stop_streaming_session,
            commands::dialogs::save_text_dialog,
            commands::settings::get_settings,
            commands::settings::set_setting,
            commands::models::list_task_models,
            commands::models::download_model,
            commands::models::delete_model,
            commands::mfu::generate_mfu,
            commands::mfu::generate_streaming_mfu,
            commands::mfu::generate_streaming_prettify,
            commands::mfu::accept_streaming_prettify,
            commands::mfu::revert_streaming_prettify
        ])
        .run(tauri::generate_context!())
        .expect("error while running WhisperPilot");
}
