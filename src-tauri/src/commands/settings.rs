//! Settings IPC commands: read all settings and update a single known key.

use crate::cloud_provider::{
    CloudProvider, CloudProviderConfiguration, CloudProviderService, KeychainCredentialStore,
};
use crate::cloud_streaming::CloudTransport;
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

fn cloud_provider_service(
    app: &tauri::AppHandle,
) -> Result<CloudProviderService<KeychainCredentialStore>> {
    Ok(CloudProviderService::new(
        &app_data_dir(app)?,
        KeychainCredentialStore,
    ))
}

/// Returns provider/model identifiers and configured status only — never API
/// key material.
#[tauri::command]
pub(crate) fn get_cloud_provider_config(
    app: tauri::AppHandle,
) -> Result<CloudProviderConfiguration> {
    cloud_provider_service(&app)?.configuration()
}

#[tauri::command]
pub(crate) fn select_cloud_provider(
    app: tauri::AppHandle,
    provider: CloudProvider,
) -> Result<CloudProviderConfiguration> {
    cloud_provider_service(&app)?.select(provider)
}

#[tauri::command]
pub(crate) async fn verify_cloud_provider_api_key(
    provider: CloudProvider,
    api_key: String,
) -> Result<()> {
    CloudTransport::verify(provider, &api_key).await
}

#[tauri::command]
pub(crate) async fn save_cloud_provider_api_key(
    app: tauri::AppHandle,
    provider: CloudProvider,
    api_key: String,
) -> Result<CloudProviderConfiguration> {
    // Enforce verification at the command boundary as well as in the form:
    // callers cannot persist a key merely by bypassing the UI's disabled Save.
    CloudTransport::verify(provider, &api_key).await?;
    cloud_provider_service(&app)?.save_api_key(provider, &api_key)
}

#[tauri::command]
pub(crate) fn remove_cloud_provider_api_key(
    app: tauri::AppHandle,
    provider: CloudProvider,
) -> Result<CloudProviderConfiguration> {
    cloud_provider_service(&app)?.remove_api_key(provider)
}
