//! AI model-management IPC commands: list the catalog, download (SHA-verified,
//! with progress), and delete downloaded model files.

use crate::error::{AppError, Result};
use crate::models;
use crate::models::TaskModel;
use crate::settings;
use crate::state::{app_data_dir, AppState};
use tauri::Emitter;

/// List the AI models catalog (transcription, diarization) with each
/// entry's current downloaded state.
#[tauri::command]
pub(crate) fn list_task_models(app: tauri::AppHandle) -> Result<Vec<TaskModel>> {
    let dir = app_data_dir(&app)?;
    Ok(models::list_task_models(&dir))
}

/// Download the catalog entry `id`, verifying SHA-256 before marking it
/// ready. Emits `model_download_progress { id, fraction, stage }` as bytes
/// arrive and again when the fetched bytes move on to hash verification.
#[tauri::command]
pub(crate) async fn download_model(app: tauri::AppHandle, id: String) -> Result<()> {
    let dir = app_data_dir(&app)?;
    let progress_app = app.clone();
    let progress_id = id.clone();
    models::download_model(&dir, &id, move |fraction, stage| {
        let _ = progress_app.emit(
            "model_download_progress",
            serde_json::json!({ "id": progress_id, "fraction": fraction, "stage": stage }),
        );
    })
    .await
}

/// Delete catalog entry `id`'s downloaded file(s), returning it to
/// not-downloaded. If `id` was the currently active diarization variant,
/// also reverts `active_model.diarization` to "none" so a later
/// transcription does not fail open against a model no longer on disk.
#[tauri::command]
pub(crate) async fn delete_model(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<()> {
    let dir = app_data_dir(&app)?;
    let is_llm = models::CATALOG
        .iter()
        .any(|entry| entry.id == id && entry.task == "llm");
    if is_llm {
        let runtime = std::sync::Arc::clone(&state.llm_runtime);
        return tokio::task::spawn_blocking(move || {
            runtime.mutate_selected_model(|| delete_llm_model_under_barrier(&dir, &id))
        })
        .await
        .map_err(|error| AppError::Llm(error.to_string()))?;
    }

    let settings = settings::get_settings(&dir);
    models::delete_model(&dir, &id)?;
    if models::delete_clears_active_diarization_variant(&id, &settings.active_model_diarization) {
        settings::set_setting(&dir, "active_model.diarization", "none")?;
    }
    Ok(())
}

/// Called only while the application's selected-model mutation barrier is
/// held. Clear an active selection before deleting its file, and restore that
/// selection if deletion fails, so persistence never points at an asset this
/// operation removed.
fn delete_llm_model_under_barrier(dir: &std::path::Path, id: &str) -> Result<()> {
    let was_active = settings::get_settings(dir).active_model_llm.as_deref() == Some(id);
    if was_active {
        settings::set_setting(dir, "active_model.llm", "")?;
    }

    if let Err(delete_error) = models::delete_model(dir, id) {
        if was_active {
            if let Err(rollback_error) = settings::set_setting(dir, "active_model.llm", id) {
                return Err(AppError::Io(format!(
                    "model deletion failed ({delete_error}); restoring the active model selection also failed ({rollback_error})"
                )));
            }
        }
        return Err(delete_error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_llm_id() -> &'static str {
        models::CATALOG
            .iter()
            .find(|entry| entry.task == "llm")
            .expect("LLM catalog entry")
            .id
    }

    #[test]
    fn failed_active_llm_deletion_restores_the_persisted_selection() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = first_llm_id();
        settings::set_setting(temp.path(), "active_model.llm", id).expect("select model");
        let asset_path = models::asset_paths(temp.path(), id)
            .expect("known model")
            .remove(0);
        std::fs::create_dir_all(&asset_path).expect("make deletion fail on a directory");

        delete_llm_model_under_barrier(temp.path(), id)
            .expect_err("remove_file must reject a directory");

        assert_eq!(
            settings::get_settings(temp.path())
                .active_model_llm
                .as_deref(),
            Some(id)
        );
    }

    #[test]
    fn successful_active_llm_deletion_clears_the_persisted_selection_first() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = first_llm_id();
        settings::set_setting(temp.path(), "active_model.llm", id).expect("select model");
        let asset_path = models::asset_paths(temp.path(), id)
            .expect("known model")
            .remove(0);
        std::fs::create_dir_all(asset_path.parent().expect("model parent"))
            .expect("create model dir");
        std::fs::write(&asset_path, b"downloaded model placeholder").expect("write model");

        delete_llm_model_under_barrier(temp.path(), id).expect("delete active model");

        assert!(!asset_path.exists());
        assert_eq!(settings::get_settings(temp.path()).active_model_llm, None);
    }
}
