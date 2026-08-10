//! Settings IPC commands: read all settings and update a single known key.

use crate::error::Result;
use crate::settings;
use crate::settings::Settings;
use crate::state::app_data_dir;

/// Read all settings (theme, ui_language, active model), applying beta
/// defaults for any key never set.
#[tauri::command]
pub(crate) fn get_settings(app: tauri::AppHandle) -> Result<Settings> {
    let dir = app_data_dir(&app)?;
    Ok(settings::get_settings(&dir))
}

/// Update one known setting (theme, ui_language, or active_model.transcription)
/// and persist it immediately; rejects an unknown key or an invalid value.
#[tauri::command]
pub(crate) fn set_setting(app: tauri::AppHandle, key: String, value: String) -> Result<Settings> {
    let dir = app_data_dir(&app)?;
    settings::set_setting(&dir, &key, &value)
}
