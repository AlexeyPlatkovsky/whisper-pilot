//! Source-contract checks for the Meeting command surface (WP-86).

const TAURI_ENTRYPOINT: &str = include_str!("../src/lib.rs");
const FRONTEND_IPC: &str = include_str!("../../src/ipc.ts");

#[test]
fn meeting_transcription_is_registered_without_a_cancel_command() {
    assert!(
        TAURI_ENTRYPOINT.contains("commands::transcription::transcribe_meeting"),
        "the Meeting transcription command must remain registered"
    );
    assert!(
        !TAURI_ENTRYPOINT.contains("cancel_transcription"),
        "the removed Meeting cancel command must not be registered"
    );
    assert!(
        !FRONTEND_IPC.contains("cancel_transcription"),
        "the frontend must not expose the removed Meeting cancel command"
    );
}
