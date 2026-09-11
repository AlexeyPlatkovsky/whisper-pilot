//! Settings IPC commands: read all settings and update a single known key.

use crate::cloud_provider::{
    CloudProvider, CloudProviderConfiguration, CloudProviderService, KeychainCredentialStore,
};
use crate::cloud_streaming::CloudTransport;
use crate::error::{AppError, Result};
use crate::models;
use crate::settings;
use crate::settings::Settings;
use crate::state::{app_data_dir, AppState};

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
pub(crate) async fn set_setting(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> Result<Settings> {
    let dir = app_data_dir(&app)?;
    if key == "active_model.llm" {
        let runtime = std::sync::Arc::clone(&state.llm_runtime);
        tokio::task::spawn_blocking(move || {
            runtime.mutate_selected_model(|| {
                ensure_llm_selection_is_downloaded(&dir, &value)?;
                settings::set_setting(&dir, &key, &value)
            })
        })
        .await
        .map_err(|error| AppError::Llm(error.to_string()))?
    } else {
        settings::set_setting(&dir, &key, &value)
    }
}

fn ensure_llm_selection_is_downloaded(dir: &std::path::Path, value: &str) -> Result<()> {
    if value.trim().is_empty()
        || models::list_task_models(dir)
            .iter()
            .any(|model| model.task == "llm" && model.id == value && model.downloaded)
    {
        return Ok(());
    }
    Err(AppError::InvalidSetting(format!(
        "local LLM model is not downloaded: {value}"
    )))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_llm_must_be_downloaded_but_clearing_selection_is_allowed() {
        let temp = tempfile::tempdir().expect("temp dir");
        let entry = models::CATALOG
            .iter()
            .find(|entry| entry.task == "llm")
            .expect("LLM catalog entry");

        ensure_llm_selection_is_downloaded(temp.path(), "")
            .expect("clearing selection must remain available");
        ensure_llm_selection_is_downloaded(temp.path(), entry.id)
            .expect_err("missing model must not become active");

        let asset = entry.assets.first().expect("LLM asset");
        let path = models::asset_paths(temp.path(), entry.id)
            .expect("known model")
            .remove(0);
        std::fs::create_dir_all(path.parent().expect("model parent")).expect("create model dir");
        let file = std::fs::File::create(path).expect("create sparse model placeholder");
        file.set_len(asset.size_bytes)
            .expect("match downloaded asset size");

        ensure_llm_selection_is_downloaded(temp.path(), entry.id)
            .expect("complete downloaded model may become active");
    }
}
