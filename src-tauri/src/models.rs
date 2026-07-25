//! AI model catalog and download: fetch from a known URL, verify SHA-256,
//! and mark ready only on success. One catalog entry (row) per task in beta
//! (F005-R3, F005-T3); an entry may bundle more than one file (diarization
//! needs a segmentation model and a speaker-embedding model).

use crate::error::{AppError, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Reports bytes downloaded so far for one asset.
type ProgressCb = Box<dyn Fn(u64) + Send + Sync>;

/// One downloadable file belonging to a catalog entry. `variant_id` is `None`
/// for an asset shared by every selectable choice in its entry (e.g. the
/// diarization segmentation model) and `Some(id)` for one user-selectable
/// choice among several sharing the same entry (e.g. a diarization embedding
/// model) — `variant_label`/`recommended` are meaningful only when
/// `variant_id` is `Some`.
#[derive(Debug, PartialEq)]
pub struct ModelAsset {
    pub url: &'static str,
    pub sha256: &'static str,
    pub size_bytes: u64,
    pub file_name: &'static str,
    pub variant_id: Option<&'static str>,
    pub variant_label: Option<&'static str>,
    pub recommended: bool,
}

/// One row in the AI models section: a task and the asset(s) it needs.
pub struct ModelCatalogEntry {
    pub id: &'static str,
    pub task: &'static str,
    pub label: &'static str,
    pub assets: &'static [ModelAsset],
}

/// Beta catalog. Every URL/SHA-256/size below was verified directly against
/// the publishing source (Hugging Face CDN headers for the Whisper model;
/// GitHub release checksum.txt / a direct hash of the downloaded archive for
/// the sherpa-onnx diarization assets) — not guessed.
pub const CATALOG: &[ModelCatalogEntry] = &[
    ModelCatalogEntry {
        id: "transcription",
        task: "transcription",
        label: "Whisper large-v3-turbo (Q8)",
        assets: &[ModelAsset {
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q8_0.bin",
            sha256: "317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1",
            size_bytes: 874_188_075,
            file_name: "ggml-large-v3-turbo-q8_0.bin",
            variant_id: None,
            variant_label: None,
            recommended: false,
        }],
    },
    ModelCatalogEntry {
        id: "diarization",
        task: "diarization",
        label: "Speaker diarization (pyannote segmentation + selectable embedding)",
        assets: &[
            ModelAsset {
                url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2",
                sha256: "24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488",
                size_bytes: 6_958_444,
                file_name: "sherpa-onnx-pyannote-segmentation-3-0.tar.bz2",
                variant_id: None,
                variant_label: None,
                recommended: false,
            },
            // Independently verified 3 ways (GitHub release API asset
            // metadata, the release's checksum.txt, and a direct
            // download+hash) during the now-superseded WP-51 experiment.
            ModelAsset {
                url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
                sha256: "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b",
                size_bytes: 29_596_978,
                file_name: "3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
                variant_id: Some("campplus"),
                variant_label: Some("CAM++"),
                recommended: false,
            },
            ModelAsset {
                url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/nemo_en_titanet_large.onnx",
                sha256: "d51abcf31717ef28162f26acb9d44dd4127c3d44c9b8624f699f3425daca8e77",
                size_bytes: 101_405_493,
                file_name: "nemo_en_titanet_large.onnx",
                variant_id: Some("titanet-large"),
                variant_label: Some("TitaNet-large"),
                recommended: true,
            },
        ],
    },
];

/// Per-task view returned to the frontend. For an entry with selectable
/// variants (diarization), one `TaskModel` row is emitted per variant rather
/// than one row for the whole entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskModel {
    pub id: String,
    pub task: String,
    pub label: String,
    pub downloaded: bool,
    pub size_bytes: u64,
    pub recommended: bool,
}

fn models_dir(app_support_dir: &Path) -> PathBuf {
    app_support_dir.join("models")
}

fn asset_path(app_support_dir: &Path, asset: &ModelAsset) -> PathBuf {
    models_dir(app_support_dir).join(asset.file_name)
}

fn is_asset_downloaded(app_support_dir: &Path, asset: &ModelAsset) -> bool {
    std::fs::metadata(asset_path(app_support_dir, asset))
        .map(|m| m.len() == asset.size_bytes)
        .unwrap_or(false)
}

fn entry_downloaded(app_support_dir: &Path, entry: &ModelCatalogEntry) -> bool {
    entry
        .assets
        .iter()
        .all(|a| is_asset_downloaded(app_support_dir, a))
}

/// The assets to operate on for one download/delete-addressable id: either a
/// whole catalog entry with no selectable variants (e.g. "transcription"), or
/// one variant within an entry that has several (e.g.
/// "diarization-titanet-large").
pub enum ResolvedTarget<'a> {
    Entry(&'a ModelCatalogEntry),
    Variant {
        shared: Vec<&'a ModelAsset>,
        own: &'a ModelAsset,
    },
}

impl<'a> ResolvedTarget<'a> {
    /// Assets that must be present for this target to count as downloaded —
    /// for a variant, the shared assets plus its own asset.
    pub fn download_assets(&self) -> Vec<&'a ModelAsset> {
        match self {
            ResolvedTarget::Entry(entry) => entry.assets.iter().collect(),
            ResolvedTarget::Variant { shared, own } => {
                let mut assets = shared.clone();
                assets.push(own);
                assets
            }
        }
    }

    /// Assets to remove on delete — for a variant, only its own asset, never
    /// a shared asset another variant may still need.
    pub fn delete_assets(&self) -> Vec<&'a ModelAsset> {
        match self {
            ResolvedTarget::Entry(entry) => entry.assets.iter().collect(),
            ResolvedTarget::Variant { own, .. } => vec![own],
        }
    }
}

/// Resolve a download/delete-addressable id to its target assets. A bare
/// catalog entry id (e.g. "transcription") matches the whole entry; a
/// synthetic `"{entry_id}-{variant_id}"` id (e.g. "diarization-campplus")
/// matches one variant within an entry that has several.
pub fn resolve_catalog_target(id: &str) -> Option<ResolvedTarget<'_>> {
    if let Some(entry) = CATALOG.iter().find(|e| e.id == id) {
        return Some(ResolvedTarget::Entry(entry));
    }
    for entry in CATALOG {
        for asset in entry.assets {
            if let Some(variant_id) = asset.variant_id {
                if id == format!("{}-{variant_id}", entry.id) {
                    let shared = entry
                        .assets
                        .iter()
                        .filter(|a| a.variant_id.is_none())
                        .collect();
                    return Some(ResolvedTarget::Variant {
                        shared,
                        own: asset,
                    });
                }
            }
        }
    }
    None
}

/// List every catalog entry with its current downloaded state. An entry with
/// selectable variants (diarization) yields one row per variant instead of
/// one row for the whole entry.
pub fn list_task_models(app_support_dir: &Path) -> Vec<TaskModel> {
    CATALOG
        .iter()
        .flat_map(|entry| {
            let variants: Vec<&ModelAsset> = entry
                .assets
                .iter()
                .filter(|a| a.variant_id.is_some())
                .collect();
            if variants.is_empty() {
                vec![TaskModel {
                    id: entry.id.to_string(),
                    task: entry.task.to_string(),
                    label: entry.label.to_string(),
                    downloaded: entry_downloaded(app_support_dir, entry),
                    size_bytes: entry.assets.iter().map(|a| a.size_bytes).sum(),
                    recommended: false,
                }]
            } else {
                let shared: Vec<&ModelAsset> = entry
                    .assets
                    .iter()
                    .filter(|a| a.variant_id.is_none())
                    .collect();
                let shared_downloaded = shared.iter().all(|a| is_asset_downloaded(app_support_dir, a));
                let shared_size: u64 = shared.iter().map(|a| a.size_bytes).sum();
                variants
                    .into_iter()
                    .map(|asset| TaskModel {
                        id: format!("{}-{}", entry.id, asset.variant_id.unwrap()),
                        task: entry.task.to_string(),
                        label: asset.variant_label.unwrap_or(entry.label).to_string(),
                        downloaded: shared_downloaded && is_asset_downloaded(app_support_dir, asset),
                        size_bytes: shared_size + asset.size_bytes,
                        recommended: asset.recommended,
                    })
                    .collect()
            }
        })
        .collect()
}

/// Delete the file(s) targeted by download/delete-addressable id `id`.
/// Idempotent: a file that is already missing is not an error. An unknown id
/// is. For a variant id, only that variant's own asset is removed — the
/// shared segmentation asset and any sibling variant's asset are untouched.
pub fn delete_model(app_support_dir: &Path, id: &str) -> Result<()> {
    let target = resolve_catalog_target(id)
        .ok_or_else(|| AppError::ModelCatalogNotFound(id.to_string()))?;

    for asset in target.delete_assets() {
        match std::fs::remove_file(asset_path(app_support_dir, asset)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(AppError::from(e)),
        }
    }
    Ok(())
}

/// Path to the first (in beta, only) asset of catalog entry `id`, if the id
/// is known. Used by transcribe.rs to load the model WP-39 downloaded.
pub fn primary_asset_path(app_support_dir: &Path, id: &str) -> Option<PathBuf> {
    let entry = CATALOG.iter().find(|e| e.id == id)?;
    entry.assets.first().map(|a| asset_path(app_support_dir, a))
}

/// Raw downloaded asset paths for catalog entry `id`, in catalog order.
/// Used by diarize.rs to locate the diarization entry's two assets (the
/// segmentation archive and the embedding model) without duplicating
/// `models_dir`/`asset_path`'s path construction.
pub fn asset_paths(app_support_dir: &Path, id: &str) -> Option<Vec<PathBuf>> {
    let entry = CATALOG.iter().find(|e| e.id == id)?;
    Some(
        entry
            .assets
            .iter()
            .map(|a| asset_path(app_support_dir, a))
            .collect(),
    )
}

async fn hash_file(path: &Path) -> Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<String> {
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)?;
        Ok(format!("{:x}", hasher.finalize()))
    })
    .await
    .map_err(|e| AppError::ModelDownload(e.to_string()))?
}

/// Download every asset in `assets` that is not already present, verify each
/// against its known SHA-256, and only then rename it into place. An asset
/// already downloaded (matching size on disk) is skipped without a network
/// call — this lets a variant download share an already-fetched shared asset
/// (e.g. the diarization segmentation model) with a sibling variant. `fetch`
/// is injected so tests never hit the real network: it must write the bytes
/// it fetched to `dest` and call `on_progress(downloaded, total)` as they
/// arrive.
async fn download_entry<F, Fut>(
    fetch: F,
    app_support_dir: &Path,
    assets: &[&ModelAsset],
    on_progress: impl Fn(f64) + Send + Sync + 'static,
) -> Result<()>
where
    F: Fn(&'static str, PathBuf, ProgressCb) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let dir = models_dir(app_support_dir);
    tokio::fs::create_dir_all(&dir).await?;

    let total: u64 = assets.iter().map(|a| a.size_bytes).sum::<u64>().max(1);
    let done_before = Arc::new(AtomicU64::new(0));
    let on_progress = Arc::new(on_progress);

    for asset in assets {
        if is_asset_downloaded(app_support_dir, asset) {
            done_before.fetch_add(asset.size_bytes, Ordering::Relaxed);
            on_progress(
                (done_before.load(Ordering::Relaxed) as f64 / total as f64).min(1.0),
            );
            continue;
        }

        let final_path = asset_path(app_support_dir, asset);
        let temp_path = dir.join(format!("{}.part", asset.file_name));

        let done_before_cl = Arc::clone(&done_before);
        let on_progress_cl = Arc::clone(&on_progress);
        let asset_size = asset.size_bytes;
        let progress_cb: ProgressCb = Box::new(move |downloaded| {
            let base = done_before_cl.load(Ordering::Relaxed);
            let fraction = (base + downloaded.min(asset_size)) as f64 / total as f64;
            on_progress_cl(fraction.min(1.0));
        });

        if let Err(e) = fetch(asset.url, temp_path.clone(), progress_cb).await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(e);
        }

        let hash = hash_file(&temp_path).await?;
        if hash != asset.sha256 {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(AppError::ModelShaMismatch {
                expected: asset.sha256.to_string(),
                actual: hash,
            });
        }

        tokio::fs::rename(&temp_path, &final_path).await?;
        done_before.fetch_add(asset.size_bytes, Ordering::Relaxed);
    }

    Ok(())
}

async fn http_fetch(url: &'static str, dest: PathBuf, on_progress: ProgressCb) -> Result<()> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let resp = reqwest::get(url)
        .await
        .map_err(|e| AppError::ModelDownload(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::ModelDownload(format!("HTTP {}", resp.status())));
    }
    let mut file = tokio::fs::File::create(&dest).await?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::ModelDownload(e.to_string()))?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded);
    }
    Ok(())
}

/// Download the target `id` resolves to for real, over HTTPS. For a variant
/// id, this fetches whichever of the shared asset or the variant's own asset
/// is not already present.
pub async fn download_model(
    app_support_dir: &Path,
    id: &str,
    on_progress: impl Fn(f64) + Send + Sync + 'static,
) -> Result<()> {
    let target =
        resolve_catalog_target(id).ok_or_else(|| AppError::ModelCatalogNotFound(id.to_string()))?;
    download_entry(
        http_fetch,
        app_support_dir,
        &target.download_assets(),
        on_progress,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_asset(sha256: &'static str, size_bytes: u64) -> ModelAsset {
        ModelAsset {
            url: "https://test.invalid/model.bin",
            sha256,
            size_bytes,
            file_name: "test-model.bin",
            variant_id: None,
            variant_label: None,
            recommended: false,
        }
    }

    fn assets_of(entry: &ModelCatalogEntry) -> Vec<&ModelAsset> {
        entry.assets.iter().collect()
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    type FetchFuture = std::pin::Pin<Box<dyn Future<Output = Result<()>> + Send>>;

    fn fetch_writing(
        content: Vec<u8>,
    ) -> impl Fn(&'static str, PathBuf, ProgressCb) -> FetchFuture {
        move |_url, dest, on_progress| {
            let content = content.clone();
            Box::pin(async move {
                tokio::fs::write(&dest, &content).await?;
                on_progress(content.len() as u64);
                Ok(())
            })
        }
    }

    fn fetch_failing() -> impl Fn(&'static str, PathBuf, ProgressCb) -> FetchFuture {
        move |_url, _dest, _on_progress| {
            Box::pin(async move { Err(AppError::ModelDownload("connection reset".into())) })
        }
    }

    #[tokio::test]
    async fn is_asset_downloaded_false_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let asset = test_asset("irrelevant", 5);

        assert!(!is_asset_downloaded(dir.path(), &asset));
    }

    #[tokio::test]
    async fn is_asset_downloaded_true_when_file_present_with_matching_size() {
        let dir = tempfile::tempdir().unwrap();
        let asset = test_asset("irrelevant", 5);
        tokio::fs::create_dir_all(models_dir(dir.path()))
            .await
            .unwrap();
        tokio::fs::write(asset_path(dir.path(), &asset), b"hello")
            .await
            .unwrap();

        assert!(is_asset_downloaded(dir.path(), &asset));
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

    fn write_sized_placeholder(path: &Path, size_bytes: u64) {
        std::fs::write(path, vec![0u8; size_bytes as usize]).unwrap();
    }

    #[tokio::test]
    async fn download_entry_verifies_and_renames_into_place_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let content = b"a real whisper model, honest".to_vec();
        let hash = sha256_hex(&content);
        let asset = test_asset(Box::leak(hash.into_boxed_str()), content.len() as u64);
        let entry = ModelCatalogEntry {
            id: "test",
            task: "test",
            label: "Test",
            assets: Box::leak(vec![asset].into_boxed_slice()),
        };

        download_entry(
            fetch_writing(content.clone()),
            dir.path(),
            &assets_of(&entry),
            |_| {},
        )
        .await
        .unwrap();

        let final_path = asset_path(dir.path(), &entry.assets[0]);
        let on_disk = tokio::fs::read(&final_path).await.unwrap();
        assert_eq!(on_disk, content);
        assert!(entry_downloaded(dir.path(), &entry));
    }

    #[tokio::test]
    async fn download_entry_rejects_sha_mismatch_and_leaves_no_file_at_final_path() {
        let dir = tempfile::tempdir().unwrap();
        let content = b"not what the catalog expects".to_vec();
        let entry = ModelCatalogEntry {
            id: "test",
            task: "test",
            label: "Test",
            assets: Box::leak(
                vec![test_asset(
                    "0000000000000000000000000000000000000000000000000000000000000000",
                    content.len() as u64,
                )]
                .into_boxed_slice(),
            ),
        };

        let err = download_entry(fetch_writing(content), dir.path(), &assets_of(&entry), |_| {})
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::ModelShaMismatch { .. }));
        assert!(!entry_downloaded(dir.path(), &entry));
        let final_path = asset_path(dir.path(), &entry.assets[0]);
        assert!(!final_path.exists());
        let temp_path = models_dir(dir.path()).join(format!("{}.part", entry.assets[0].file_name));
        assert!(
            !temp_path.exists(),
            "temp file must be cleaned up after a SHA mismatch"
        );
    }

    #[tokio::test]
    async fn download_entry_propagates_network_failure_without_creating_final_file() {
        let dir = tempfile::tempdir().unwrap();
        let entry = ModelCatalogEntry {
            id: "test",
            task: "test",
            label: "Test",
            assets: Box::leak(vec![test_asset("irrelevant", 10)].into_boxed_slice()),
        };

        let err = download_entry(fetch_failing(), dir.path(), &assets_of(&entry), |_| {})
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::ModelDownload(_)));
        assert!(!entry_downloaded(dir.path(), &entry));
    }

    #[tokio::test]
    async fn download_entry_skips_an_asset_that_is_already_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let already_present = test_asset("irrelevant-since-skipped", 5);
        std::fs::create_dir_all(models_dir(dir.path())).unwrap();
        std::fs::write(asset_path(dir.path(), &already_present), b"aaaaa").unwrap();
        let missing_content = b"the missing one".to_vec();
        let missing = ModelAsset {
            url: "https://test.invalid/missing.bin",
            sha256: Box::leak(sha256_hex(&missing_content).into_boxed_str()),
            size_bytes: missing_content.len() as u64,
            file_name: "missing.bin",
            variant_id: None,
            variant_label: None,
            recommended: false,
        };

        // A fetch that errors if invoked for the already-downloaded asset,
        // proving it was skipped rather than re-fetched.
        let missing_content_cl = missing_content.clone();
        let fetch = move |url: &'static str, dest: PathBuf, on_progress: ProgressCb| {
            let missing_content = missing_content_cl.clone();
            Box::pin(async move {
                if url.ends_with("model.bin") {
                    panic!("already-downloaded asset must not be re-fetched");
                }
                tokio::fs::write(&dest, &missing_content).await?;
                on_progress(missing_content.len() as u64);
                Ok(())
            }) as FetchFuture
        };

        download_entry(
            fetch,
            dir.path(),
            &[&already_present, &missing],
            |_| {},
        )
        .await
        .unwrap();

        assert_eq!(
            std::fs::read(asset_path(dir.path(), &already_present)).unwrap(),
            b"aaaaa",
            "the already-downloaded asset's bytes must be untouched"
        );
        assert!(is_asset_downloaded(dir.path(), &missing));
    }

    #[tokio::test]
    async fn download_model_rejects_unknown_id() {
        let dir = tempfile::tempdir().unwrap();

        let err = download_model(dir.path(), "not-a-real-model", |_| {})
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::ModelCatalogNotFound(_)));
    }

    #[tokio::test]
    async fn download_entry_reports_fractions_proportional_to_each_assets_share() {
        let dir = tempfile::tempdir().unwrap();
        let content_a = vec![0u8; 10];
        let content_b = vec![1u8; 30];
        let hash_a = sha256_hex(&content_a);
        let hash_b = sha256_hex(&content_b);
        let asset_a = ModelAsset {
            url: "https://test.invalid/a.bin",
            sha256: Box::leak(hash_a.into_boxed_str()),
            size_bytes: content_a.len() as u64,
            file_name: "a.bin",
            variant_id: None,
            variant_label: None,
            recommended: false,
        };
        let asset_b = ModelAsset {
            url: "https://test.invalid/b.bin",
            sha256: Box::leak(hash_b.into_boxed_str()),
            size_bytes: content_b.len() as u64,
            file_name: "b.bin",
            variant_id: None,
            variant_label: None,
            recommended: false,
        };
        let entry = ModelCatalogEntry {
            id: "bundle",
            task: "test",
            label: "Bundle",
            assets: Box::leak(vec![asset_a, asset_b].into_boxed_slice()),
        };

        let content_a_for_fetch = content_a.clone();
        let content_b_for_fetch = content_b.clone();
        let fetch = move |url: &'static str, dest: PathBuf, on_progress: ProgressCb| {
            let content = if url.ends_with("a.bin") {
                content_a_for_fetch.clone()
            } else {
                content_b_for_fetch.clone()
            };
            Box::pin(async move {
                tokio::fs::write(&dest, &content).await?;
                on_progress(content.len() as u64);
                Ok(())
            })
        };

        let fractions: Arc<std::sync::Mutex<Vec<f64>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let fractions_cl = Arc::clone(&fractions);

        download_entry(fetch, dir.path(), &assets_of(&entry), move |f| {
            fractions_cl.lock().unwrap().push(f);
        })
        .await
        .unwrap();

        // Total is 40 bytes; asset a (10 bytes) completing in one chunk reports
        // 10/40, then asset b (30 bytes) completing reports (10+30)/40 = 1.0.
        let observed = fractions.lock().unwrap().clone();
        assert_eq!(observed, vec![0.25, 1.0]);
    }

    #[tokio::test]
    async fn delete_model_removes_file_and_list_reports_not_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::create_dir_all(models_dir(dir.path()))
            .await
            .unwrap();
        let path = models_dir(dir.path()).join("ggml-large-v3-turbo-q8_0.bin");
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

    #[tokio::test]
    async fn download_model_rejects_unknown_diarization_variant_id() {
        let dir = tempfile::tempdir().unwrap();

        let err = download_model(dir.path(), "diarization-not-a-real-variant", |_| {})
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::ModelCatalogNotFound(_)));
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
        let target =
            resolve_catalog_target("transcription").expect("known entry id must resolve");

        assert_eq!(target.download_assets(), vec![&CATALOG[0].assets[0]]);
        assert_eq!(target.delete_assets(), vec![&CATALOG[0].assets[0]]);
    }

    #[test]
    fn resolve_catalog_target_returns_none_for_an_unknown_id() {
        assert!(resolve_catalog_target("not-a-real-id").is_none());
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
}
