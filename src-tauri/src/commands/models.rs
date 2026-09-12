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

    let is_asr = crate::asr::spec_by_id(&id).is_some();
    let _asr_mutation = if is_asr {
        Some(state.recorder_asr_mutation.lock().await)
    } else {
        None
    };
    #[cfg(target_os = "macos")]
    if asr_delete_is_blocked(
        is_asr,
        crate::streaming_session::current_whisper_user(&state.whisper_busy),
    ) {
        return Err(AppError::InvalidSetting(
            "ASR assets cannot be deleted while transcription or capture is active".into(),
        ));
    }

    let settings = settings::get_settings(&dir);
    let reset_transcription_model = settings.active_model_transcription.as_deref() == Some(&id);
    let reset_to_whisper = reset_transcription_model && id != crate::asr::DEFAULT_ASR_MODEL_ID;
    let deletion_result = if reset_to_whisper {
        delete_selected_asr_under_barrier(&dir, &id)
    } else {
        models::delete_model(&dir, &id)
    };
    #[cfg(target_os = "macos")]
    if crate::asr::spec_by_id(&id)
        .is_some_and(|spec| spec.engine == crate::asr::AsrEngine::Qwen3Asr)
    {
        state.clear_qwen_asr_model().await;
    }
    deletion_result?;
    if models::delete_clears_active_diarization_variant(&id, &settings.active_model_diarization) {
        settings::set_setting(&dir, "active_model.diarization", "none")?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn asr_delete_is_blocked(
    is_asr: bool,
    holder: Option<crate::streaming_session::WhisperUser>,
) -> bool {
    is_asr && holder.is_some()
}

/// Keep the safe Whisper selection after any delete failure. A multi-asset
/// bundle can fail after an earlier asset was already removed, so restoring
/// Qwen here could persist a selection that can no longer be loaded.
fn delete_selected_asr_under_barrier(dir: &std::path::Path, id: &str) -> Result<()> {
    settings::set_setting(
        dir,
        "active_model.transcription",
        crate::asr::DEFAULT_ASR_MODEL_ID,
    )?;
    models::delete_model(dir, id).map_err(|delete_error| {
        AppError::Io(format!(
            "ASR deletion failed and the affected selection remains Whisper until the complete bundle is downloaded again: {delete_error}"
        ))
    })
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

    #[test]
    fn partial_asr_bundle_deletion_keeps_the_safe_whisper_selection() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = crate::asr::QWEN3_ASR_17_GGUF_MODEL_ID;
        settings::set_setting(temp.path(), "active_model.transcription", id).expect("select Qwen");
        let paths = models::asset_paths(temp.path(), id).expect("Qwen paths");
        std::fs::create_dir_all(paths[0].parent().expect("model parent"))
            .expect("create model dir");
        std::fs::write(&paths[0], b"weights placeholder").expect("write first asset");
        std::fs::create_dir_all(&paths[1]).expect("make second asset deletion fail");

        delete_selected_asr_under_barrier(temp.path(), id)
            .expect_err("second asset directory must fail remove_file");

        assert!(!paths[0].exists(), "the first asset was already removed");
        assert_eq!(
            settings::get_settings(temp.path())
                .active_model_transcription
                .as_deref(),
            Some(crate::asr::DEFAULT_ASR_MODEL_ID)
        );
    }

    #[test]
    fn deleting_qwen_17_resets_the_shared_selection_to_whisper() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = crate::asr::QWEN3_ASR_17_GGUF_MODEL_ID;
        settings::set_setting(temp.path(), "active_model.transcription", id)
            .expect("select Qwen for all modes");

        delete_selected_asr_under_barrier(temp.path(), id)
            .expect("delete absent bundle idempotently");

        let current = settings::get_settings(temp.path());
        assert_eq!(
            current.active_model_transcription.as_deref(),
            Some(crate::asr::DEFAULT_ASR_MODEL_ID)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn every_active_transcription_pipeline_blocks_asr_delete() {
        assert!(asr_delete_is_blocked(
            true,
            Some(crate::streaming_session::WhisperUser::Recorder)
        ));
        assert!(asr_delete_is_blocked(
            true,
            Some(crate::streaming_session::WhisperUser::Streaming)
        ));
        assert!(asr_delete_is_blocked(
            true,
            Some(crate::streaming_session::WhisperUser::Meeting)
        ));
        assert!(!asr_delete_is_blocked(true, None));
    }
}
