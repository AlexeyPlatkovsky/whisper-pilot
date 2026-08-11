//! AI model-management IPC commands: list the catalog, download (SHA-verified,
//! with progress), and delete downloaded model files.

use crate::error::Result;
use crate::models;
use crate::models::TaskModel;
use crate::settings;
use crate::state::app_data_dir;
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
pub(crate) fn delete_model(app: tauri::AppHandle, id: String) -> Result<()> {
    let dir = app_data_dir(&app)?;
    models::delete_model(&dir, &id)?;

    let settings = settings::get_settings(&dir);
    if models::delete_clears_active_diarization_variant(&id, &settings.active_model_diarization) {
        settings::set_setting(&dir, "active_model.diarization", "none")?;
    }
    if settings.active_model_llm.as_deref() == Some(&id) {
        settings::set_setting(&dir, "active_model.llm", "")?;
    }
    Ok(())
}
