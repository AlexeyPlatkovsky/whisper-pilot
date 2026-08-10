//! AI model catalog and download: fetch from a known URL, verify SHA-256,
//! and mark ready only on success. One catalog entry (row) per task in beta
//! (F005-R3, F005-T3); an entry may bundle more than one file (diarization
//! needs a segmentation model and a speaker-embedding model).
//!
//! The static catalog and on-disk path/state resolution live in
//! [`catalog`]; the download machinery (fetch, SHA verify, throttled
//! progress) lives in [`download`].

pub(crate) mod catalog;
pub(crate) mod download;

pub use catalog::{
    asset_paths, delete_clears_active_diarization_variant, delete_model,
    is_diarization_variant_downloaded, list_task_models, primary_asset_path,
    resolve_catalog_target, ModelAsset, ModelCatalogEntry, ResolvedTarget, TaskModel, CATALOG,
};
pub use download::{download_model, DownloadStage};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use std::path::Path;

    fn write_sized_placeholder(path: &Path, size_bytes: u64) {
        std::fs::write(path, vec![0u8; size_bytes as usize]).unwrap();
    }

    #[tokio::test]
    async fn list_task_models_reports_transcription_and_both_diarization_variants_not_downloaded_by_default(
    ) {
        let dir = tempfile::tempdir().unwrap();

        let models = list_task_models(dir.path());

        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"transcription"));
        assert!(ids.contains(&"diarization-campplus"));
        assert!(ids.contains(&"diarization-titanet-large"));
        assert!(models.iter().all(|m| !m.downloaded));
    }

    #[tokio::test]
    async fn list_task_models_emits_one_row_per_diarization_variant_with_recommended_flag() {
        let dir = tempfile::tempdir().unwrap();

        let models = list_task_models(dir.path());

        let campplus = models
            .iter()
            .find(|m| m.id == "diarization-campplus")
            .expect("campplus variant row");
        let titanet = models
            .iter()
            .find(|m| m.id == "diarization-titanet-large")
            .expect("titanet-large variant row");
        assert_eq!(campplus.task, "diarization");
        assert_eq!(titanet.task, "diarization");
        assert!(!campplus.recommended);
        assert!(titanet.recommended);
    }

    #[tokio::test]
    async fn list_task_models_diarization_variant_downloaded_requires_both_shared_segmentation_and_its_own_embedding(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let paths = asset_paths(dir.path(), "diarization").unwrap();
        // paths[0] = segmentation archive (shared), paths[1] = campplus embedding,
        // paths[2] = titanet-large embedding, per the restructured 3-asset entry.
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        write_sized_placeholder(&paths[0], CATALOG[1].assets[0].size_bytes);
        write_sized_placeholder(&paths[1], CATALOG[1].assets[1].size_bytes);
        // titanet-large's own embedding (paths[2]) intentionally left missing.

        let models = list_task_models(dir.path());

        let campplus = models
            .iter()
            .find(|m| m.id == "diarization-campplus")
            .unwrap();
        let titanet = models
            .iter()
            .find(|m| m.id == "diarization-titanet-large")
            .unwrap();
        assert!(
            campplus.downloaded,
            "campplus variant should be downloaded once the shared segmentation model and its own embedding are both present"
        );
        assert!(
            !titanet.downloaded,
            "titanet-large variant should not be downloaded while its own embedding is missing, even though the shared segmentation model is present"
        );
    }

    #[tokio::test]
    async fn delete_model_removes_file_and_list_reports_not_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::create_dir_all(catalog::models_dir(dir.path()))
            .await
            .unwrap();
        let path = catalog::models_dir(dir.path()).join("ggml-large-v3-turbo-q8_0.bin");
        tokio::fs::write(&path, b"placeholder").await.unwrap();

        delete_model(dir.path(), "transcription").unwrap();

        assert!(!path.exists());
        let models = list_task_models(dir.path());
        let transcription = models.iter().find(|m| m.id == "transcription").unwrap();
        assert!(!transcription.downloaded);
    }

    #[tokio::test]
    async fn delete_model_is_idempotent_when_file_already_missing() {
        let dir = tempfile::tempdir().unwrap();

        delete_model(dir.path(), "transcription").unwrap();
    }

    #[tokio::test]
    async fn delete_model_rejects_unknown_id() {
        let dir = tempfile::tempdir().unwrap();

        let err = delete_model(dir.path(), "not-a-real-model").unwrap_err();

        assert!(matches!(err, AppError::ModelCatalogNotFound(_)));
    }

    #[tokio::test]
    async fn delete_model_rejects_unknown_diarization_variant_id() {
        let dir = tempfile::tempdir().unwrap();

        let err = delete_model(dir.path(), "diarization-not-a-real-variant").unwrap_err();

        assert!(matches!(err, AppError::ModelCatalogNotFound(_)));
    }

    #[tokio::test]
    async fn delete_model_removes_only_the_targeted_variants_embedding_leaving_shared_segmentation_and_sibling_variant_untouched(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let paths = asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        write_sized_placeholder(&paths[0], CATALOG[1].assets[0].size_bytes);
        write_sized_placeholder(&paths[1], CATALOG[1].assets[1].size_bytes);
        write_sized_placeholder(&paths[2], CATALOG[1].assets[2].size_bytes);

        delete_model(dir.path(), "diarization-titanet-large").unwrap();

        assert!(
            paths[0].exists(),
            "shared segmentation model must survive deleting one variant"
        );
        assert!(
            paths[1].exists(),
            "the sibling (campplus) variant's embedding must survive"
        );
        assert!(
            !paths[2].exists(),
            "the targeted (titanet-large) variant's own embedding must be removed"
        );
        let models = list_task_models(dir.path());
        assert!(
            models
                .iter()
                .find(|m| m.id == "diarization-campplus")
                .unwrap()
                .downloaded
        );
        assert!(
            !models
                .iter()
                .find(|m| m.id == "diarization-titanet-large")
                .unwrap()
                .downloaded
        );
    }

    #[test]
    fn resolve_catalog_target_for_a_variant_id_includes_the_shared_asset_and_only_its_own_embedding(
    ) {
        let target = resolve_catalog_target("diarization-titanet-large")
            .expect("known variant id must resolve");
        let download_assets = target.download_assets();

        assert_eq!(download_assets.len(), 2);
        assert!(download_assets
            .iter()
            .any(|a| a.variant_id.is_none() && a.file_name.ends_with(".tar.bz2")));
        assert!(download_assets
            .iter()
            .any(|a| a.variant_id == Some("titanet-large")));
        assert!(!download_assets
            .iter()
            .any(|a| a.variant_id == Some("campplus")));
        assert_eq!(target.delete_assets(), vec![&CATALOG[1].assets[2]]);
    }

    #[test]
    fn resolve_catalog_target_for_a_whole_entry_id_returns_every_asset() {
        let target = resolve_catalog_target("transcription").expect("known entry id must resolve");

        assert_eq!(target.download_assets(), vec![&CATALOG[0].assets[0]]);
        assert_eq!(target.delete_assets(), vec![&CATALOG[0].assets[0]]);
    }

    #[test]
    fn resolve_catalog_target_returns_none_for_an_unknown_id() {
        assert!(resolve_catalog_target("not-a-real-id").is_none());
    }

    #[test]
    fn delete_clears_active_diarization_variant_true_when_deleted_id_is_the_active_variant() {
        assert!(delete_clears_active_diarization_variant(
            "diarization-titanet-large",
            "titanet-large",
        ));
    }

    #[test]
    fn delete_clears_active_diarization_variant_false_for_a_different_variant() {
        assert!(!delete_clears_active_diarization_variant(
            "diarization-campplus",
            "titanet-large",
        ));
    }

    #[test]
    fn delete_clears_active_diarization_variant_false_when_active_is_none() {
        assert!(!delete_clears_active_diarization_variant(
            "diarization-titanet-large",
            "none",
        ));
    }

    #[test]
    fn delete_clears_active_diarization_variant_false_for_a_whole_entry_id() {
        assert!(!delete_clears_active_diarization_variant(
            "transcription",
            "titanet-large",
        ));
    }

    #[test]
    fn delete_clears_active_diarization_variant_false_for_an_unknown_id() {
        assert!(!delete_clears_active_diarization_variant(
            "not-a-real-id",
            "titanet-large",
        ));
    }

    #[test]
    fn primary_asset_path_resolves_transcription_asset_under_models_dir() {
        let dir = tempfile::tempdir().unwrap();

        let path = primary_asset_path(dir.path(), "transcription").unwrap();

        assert_eq!(
            path,
            dir.path()
                .join("models")
                .join("ggml-large-v3-turbo-q8_0.bin")
        );
    }

    #[test]
    fn primary_asset_path_none_for_unknown_id() {
        let dir = tempfile::tempdir().unwrap();

        assert!(primary_asset_path(dir.path(), "not-a-real-model").is_none());
    }

    // --- DoD C-3: variant downloaded check for the fallback ---------------

    #[test]
    fn is_diarization_variant_downloaded_false_when_no_asset_is_present() {
        let dir = tempfile::tempdir().unwrap();

        assert!(!is_diarization_variant_downloaded(dir.path(), "campplus"));
    }

    #[test]
    fn is_diarization_variant_downloaded_false_when_only_shared_segmentation_is_present() {
        let dir = tempfile::tempdir().unwrap();
        let paths = asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        write_sized_placeholder(&paths[0], CATALOG[1].assets[0].size_bytes);

        assert!(!is_diarization_variant_downloaded(dir.path(), "campplus"));
    }

    #[test]
    fn is_diarization_variant_downloaded_true_when_both_shared_and_variant_asset_are_present() {
        let dir = tempfile::tempdir().unwrap();
        let paths = asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        // paths[0] = segmentation (shared), paths[1] = campplus, paths[2] = titanet-large
        write_sized_placeholder(&paths[0], CATALOG[1].assets[0].size_bytes);
        write_sized_placeholder(&paths[1], CATALOG[1].assets[1].size_bytes);

        assert!(is_diarization_variant_downloaded(dir.path(), "campplus"));
        assert!(!is_diarization_variant_downloaded(
            dir.path(),
            "titanet-large"
        ));
    }

    #[test]
    fn is_diarization_variant_downloaded_false_for_unknown_variant() {
        let dir = tempfile::tempdir().unwrap();
        let paths = asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        write_sized_placeholder(&paths[0], CATALOG[1].assets[0].size_bytes);

        assert!(!is_diarization_variant_downloaded(
            dir.path(),
            "not-a-variant"
        ));
    }
}
