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
use serde::Serialize;
#[cfg(target_os = "macos")]
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

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
    } else if key == "active_model.recorder" || key == "recorder_language" {
        let _asr_mutation = state.recorder_asr_mutation.lock().await;
        #[cfg(target_os = "macos")]
        {
            if crate::streaming_session::current_whisper_user(&state.whisper_busy)
                == Some(crate::streaming_session::WhisperUser::Recorder)
            {
                return Err(AppError::InvalidSetting(
                    "Recorder ASR and language cannot change until capture and finalization have fully ended"
                        .into(),
                ));
            }
            let snapshot = state
                .live_capture
                .lock()
                .map_err(|_| AppError::InvalidSetting("live capture lock is poisoned".into()))?
                .snapshot();
            if snapshot.source == Some(crate::live_capture::LiveCaptureSource::Recorder)
                && !matches!(
                    snapshot.phase,
                    crate::live_capture::LiveCapturePhase::Idle
                        | crate::live_capture::LiveCapturePhase::Error
                )
            {
                return Err(AppError::InvalidSetting(
                    "Recorder ASR cannot change while a session is active".into(),
                ));
            }
        }
        if key == "active_model.recorder" {
            ensure_asr_selection_is_downloaded(&dir, &value)?;
        }
        let updated = settings::set_setting(&dir, &key, &value)?;
        #[cfg(target_os = "macos")]
        if key == "active_model.recorder" {
            state.clear_qwen_asr_model().await;
        }
        Ok(updated)
    } else {
        settings::set_setting(&dir, &key, &value)
    }
}

fn ensure_asr_selection_is_downloaded(dir: &std::path::Path, value: &str) -> Result<()> {
    if models::list_task_models(dir)
        .iter()
        .any(|model| model.task == "transcription" && model.id == value && model.downloaded)
    {
        return Ok(());
    }
    Err(AppError::InvalidSetting(format!(
        "Recorder transcription model is not downloaded: {value}"
    )))
}

#[tauri::command]
pub(crate) fn set_recorder_shortcut(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    value: String,
) -> Result<Settings> {
    let canonical = crate::recorder_shortcut::RecorderShortcut::parse(&value)
        .map_err(AppError::InvalidSetting)?;
    #[cfg(target_os = "macos")]
    {
        let replacement = platform_shortcut(canonical.as_str())?;
        let mut current = state
            .registered_recorder_shortcut
            .lock()
            .map_err(|_| AppError::InvalidSetting("Recorder shortcut lock is poisoned".into()))?;
        if current.as_ref() == Some(&replacement) {
            let persisted = settings::set_setting(
                &app_data_dir(&app)?,
                "recorder_shortcut",
                canonical.as_str(),
            )?;
            set_shortcut_error(&state, None);
            return Ok(persisted);
        }
        if let Err(error) = app.global_shortcut().register(replacement) {
            let message = format!("Recorder shortcut conflict: {error}");
            set_shortcut_error(&state, Some(message.clone()));
            return Err(AppError::InvalidSetting(message));
        }
        if let Some(previous) = current.as_ref() {
            if previous != &replacement {
                if let Err(error) = app.global_shortcut().unregister(*previous) {
                    let replacement_removed = app.global_shortcut().unregister(replacement).is_ok();
                    let message = if replacement_removed {
                        format!(
                            "Recorder shortcut was not changed because the previous shortcut could not be removed: {error}"
                        )
                    } else {
                        *current = None;
                        format!(
                            "Recorder shortcut was disabled after shortcut rollback failed: {error}"
                        )
                    };
                    set_shortcut_error(&state, Some(message.clone()));
                    return Err(AppError::InvalidSetting(message));
                }
            }
        }
        let persisted = match settings::set_setting(
            &app_data_dir(&app)?,
            "recorder_shortcut",
            canonical.as_str(),
        ) {
            Ok(value) => value,
            Err(error) => {
                let replacement_removed = app
                    .global_shortcut()
                    .unregister(replacement)
                    .map(|()| true)
                    .unwrap_or(false);
                let previous = current.as_ref().copied();
                let previous_restored = previous
                    .map(|shortcut| app.global_shortcut().register(shortcut).is_ok())
                    .unwrap_or(true);
                if replacement_removed && previous_restored {
                    *current = previous;
                    set_shortcut_error(&state, Some(error.to_string()));
                } else {
                    *current = None;
                    set_shortcut_error(
                        &state,
                        Some(format!(
                            "Recorder shortcut was disabled after settings rollback failed: {error}"
                        )),
                    );
                }
                return Err(error);
            }
        };
        *current = Some(replacement);
        set_shortcut_error(&state, None);
        Ok(persisted)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = state;
        settings::set_setting(
            &app_data_dir(&app)?,
            "recorder_shortcut",
            canonical.as_str(),
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecorderShortcutStatus {
    configured: String,
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tauri::command]
pub(crate) fn get_recorder_shortcut_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RecorderShortcutStatus> {
    let configured = settings::get_settings(&app_data_dir(&app)?).recorder_shortcut;
    #[cfg(target_os = "macos")]
    {
        let active = state
            .registered_recorder_shortcut
            .lock()
            .map_err(|_| AppError::InvalidSetting("Recorder shortcut lock is poisoned".into()))?
            .is_some();
        let error = state
            .recorder_shortcut_error
            .lock()
            .map_err(|_| AppError::InvalidSetting("Recorder shortcut lock is poisoned".into()))?
            .clone();
        Ok(RecorderShortcutStatus {
            configured,
            active,
            error,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = state;
        Ok(RecorderShortcutStatus {
            configured,
            active: false,
            error: Some("Global Recorder shortcuts are available only on macOS".into()),
        })
    }
}

#[cfg(target_os = "macos")]
fn set_shortcut_error(state: &AppState, error: Option<String>) {
    if let Ok(mut current) = state.recorder_shortcut_error.lock() {
        *current = error;
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_shortcut(value: &str) -> Result<Shortcut> {
    value
        .replace("Control", "Ctrl")
        .replace("Option", "Alt")
        .replace("Command", "Super")
        .parse::<Shortcut>()
        .map_err(|error| AppError::InvalidSetting(format!("invalid Recorder shortcut: {error}")))
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

    #[test]
    fn recorder_asr_selection_requires_the_complete_bundle() {
        let temp = tempfile::tempdir().expect("temp dir");
        let entry = models::CATALOG
            .iter()
            .find(|entry| entry.id == crate::asr::QWEN3_ASR_06_MODEL_ID)
            .expect("Qwen ASR catalog entry");
        ensure_asr_selection_is_downloaded(temp.path(), entry.id)
            .expect_err("missing bundle must not become active");

        for (path, asset) in models::asset_paths(temp.path(), entry.id)
            .unwrap()
            .into_iter()
            .zip(entry.assets)
        {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let file = std::fs::File::create(path).unwrap();
            file.set_len(asset.size_bytes).unwrap();
        }

        ensure_asr_selection_is_downloaded(temp.path(), entry.id)
            .expect("complete bundle may become active");
    }
}
