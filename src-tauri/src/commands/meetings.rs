//! Meeting-library IPC commands: create/list/open/rename/delete meetings and
//! auto-save transcript segment and notes edits.

use crate::error::Result;
use crate::meetings::{MeetingDto, MeetingSummaryDto};
use crate::state::{app_data_dir, now_ms};
use crate::store::MeetingNotes;

/// Create and return an empty persisted meeting. Attaching a file and starting
/// transcription are separate, explicit actions.
#[tauri::command]
pub(crate) fn create_meeting(app: tauri::AppHandle) -> Result<MeetingDto> {
    crate::meetings::create_empty_meeting(&app_data_dir(&app)?, now_ms()?)
}

/// List persisted meetings newest first for the library sidebar.
#[tauri::command]
pub(crate) fn list_meetings(app: tauri::AppHandle) -> Result<Vec<MeetingSummaryDto>> {
    crate::meetings::list_meetings(&app_data_dir(&app)?)
}

/// Open a complete persisted meeting for the active workspace.
#[tauri::command]
pub(crate) fn open_meeting(app: tauri::AppHandle, id: i64) -> Result<MeetingDto> {
    crate::meetings::open_meeting(&app_data_dir(&app)?, id)
}

#[tauri::command]
pub(crate) fn rename_meeting(app: tauri::AppHandle, id: i64, title: String) -> Result<MeetingDto> {
    crate::meetings::rename_meeting(&app_data_dir(&app)?, id, title)
}

#[tauri::command]
pub(crate) fn delete_meeting(app: tauri::AppHandle, id: i64) -> Result<()> {
    crate::meetings::delete_meeting(&app_data_dir(&app)?, id)
}

/// Auto-save a single transcript segment's edited text. `index` addresses the
/// meeting's currently displayed (speaker-coalesced) segment list, matching
/// what the workspace renders — see `meetings::update_segment`.
#[tauri::command]
pub(crate) fn update_segment(
    app: tauri::AppHandle,
    id: i64,
    index: usize,
    text: String,
) -> Result<MeetingDto> {
    crate::meetings::update_segment(&app_data_dir(&app)?, id, index, text)
}

/// Auto-save the meeting notes fields as the user edits them.
#[tauri::command]
pub(crate) fn update_notes(app: tauri::AppHandle, notes: MeetingNotes) -> Result<MeetingDto> {
    crate::meetings::update_notes(&app_data_dir(&app)?, notes)
}
