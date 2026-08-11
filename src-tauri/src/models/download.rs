//! Model download: fetch from a known URL, verify SHA-256, and mark ready
//! only on success. The catalog's verified URLs and hashes live in `catalog`.

use crate::error::{AppError, Result};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::catalog::{asset_path, is_asset_downloaded, models_dir, ModelAsset};

/// Reports bytes downloaded so far for one asset.
type ProgressCb = Box<dyn Fn(u64) + Send + Sync>;

/// Which part of a download the reported fraction belongs to. Hashing a
/// multi-hundred-megabyte model runs long after its last byte arrives, so the
/// two phases are reported separately rather than as one "downloading" span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStage {
    Downloading,
    Verifying,
}

use serde::Serialize;

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
    on_progress: impl Fn(f64, DownloadStage) + Send + Sync + 'static,
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
                DownloadStage::Downloading,
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
            on_progress_cl(fraction.min(1.0), DownloadStage::Downloading);
        });

        if let Err(e) = fetch(asset.url, temp_path.clone(), progress_cb).await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(e);
        }

        let fetched = done_before.load(Ordering::Relaxed) + asset.size_bytes;
        on_progress(
            (fetched as f64 / total as f64).min(1.0),
            DownloadStage::Verifying,
        );

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

/// Forward a progress update only when it tells the caller something new: a
/// stage change, or at least half a percent of transfer since the last one. A
/// near-gigabyte download arrives in tens of thousands of network chunks, and
/// relaying every one of them floods whatever sink is on the other side.
fn throttled(
    sink: impl Fn(f64, DownloadStage) + Send + Sync + 'static,
) -> impl Fn(f64, DownloadStage) + Send + Sync + 'static {
    const MIN_PERMILLE_STEP: u64 = 5;
    let last_permille = AtomicU64::new(u64::MAX);
    move |fraction, stage| {
        let permille = (fraction * 1000.0).round() as u64;
        if stage == DownloadStage::Downloading
            && permille.abs_diff(last_permille.load(Ordering::Relaxed)) < MIN_PERMILLE_STEP
        {
            return;
        }
        last_permille.store(permille, Ordering::Relaxed);
        sink(fraction, stage);
    }
}

/// Download the target `id` resolves to for real, over HTTPS. For a variant
/// id, this fetches whichever of the shared asset or the variant's own asset
/// is not already present.
pub async fn download_model(
    app_support_dir: &Path,
    id: &str,
    on_progress: impl Fn(f64, DownloadStage) + Send + Sync + 'static,
) -> Result<()> {
    let target = super::catalog::resolve_catalog_target(id)
        .ok_or_else(|| AppError::ModelCatalogNotFound(id.to_string()))?;
    download_entry(
        http_fetch,
        app_support_dir,
        &target.download_assets(),
        throttled(on_progress),
    )
    .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::models::catalog::{entry_downloaded, ModelAsset, ModelCatalogEntry};
    use std::future::Future;

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
            |_, _| {},
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

        let err = download_entry(
            fetch_writing(content),
            dir.path(),
            &assets_of(&entry),
            |_, _| {},
        )
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

        let err = download_entry(fetch_failing(), dir.path(), &assets_of(&entry), |_, _| {})
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

        download_entry(fetch, dir.path(), &[&already_present, &missing], |_, _| {})
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

        let err = download_model(dir.path(), "not-a-real-model", |_, _| {})
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

        download_entry(fetch, dir.path(), &assets_of(&entry), move |f, stage| {
            if stage == DownloadStage::Downloading {
                fractions_cl.lock().unwrap().push(f);
            }
        })
        .await
        .unwrap();

        // Total is 40 bytes; asset a (10 bytes) completing in one chunk reports
        // 10/40, then asset b (30 bytes) completing reports (10+30)/40 = 1.0.
        let observed = fractions.lock().unwrap().clone();
        assert_eq!(observed, vec![0.25, 1.0]);
    }

    // Hashing a multi-hundred-megabyte model takes long enough after the last
    // byte lands that the UI has to be able to tell the two apart.
    #[tokio::test]
    async fn download_entry_reports_a_verifying_stage_once_an_assets_bytes_are_complete() {
        let dir = tempfile::tempdir().unwrap();
        let content = b"a real whisper model, honest".to_vec();
        let hash = sha256_hex(&content);
        let entry = ModelCatalogEntry {
            id: "test",
            task: "test",
            label: "Test",
            assets: Box::leak(
                vec![test_asset(
                    Box::leak(hash.into_boxed_str()),
                    content.len() as u64,
                )]
                .into_boxed_slice(),
            ),
        };

        let observed: Arc<std::sync::Mutex<Vec<(f64, DownloadStage)>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed_cl = Arc::clone(&observed);

        download_entry(
            fetch_writing(content),
            dir.path(),
            &assets_of(&entry),
            move |fraction, stage| {
                observed_cl.lock().unwrap().push((fraction, stage));
            },
        )
        .await
        .unwrap();

        assert_eq!(
            observed.lock().unwrap().clone(),
            vec![
                (1.0, DownloadStage::Downloading),
                (1.0, DownloadStage::Verifying),
            ]
        );
    }

    #[test]
    fn throttled_coalesces_intermediate_download_updates() {
        let observed: Arc<std::sync::Mutex<Vec<(f64, DownloadStage)>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed_cl = Arc::clone(&observed);
        let sink = throttled(move |fraction, stage| {
            observed_cl.lock().unwrap().push((fraction, stage));
        });

        // 0.001 apart: only the first crosses the emit threshold from nothing.
        for step in 0..=6 {
            sink(f64::from(step) / 1000.0, DownloadStage::Downloading);
        }

        assert_eq!(
            observed.lock().unwrap().clone(),
            vec![
                (0.0, DownloadStage::Downloading),
                (0.005, DownloadStage::Downloading),
            ]
        );
    }

    #[test]
    fn throttled_always_forwards_the_verifying_stage() {
        let observed: Arc<std::sync::Mutex<Vec<(f64, DownloadStage)>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed_cl = Arc::clone(&observed);
        let sink = throttled(move |fraction, stage| {
            observed_cl.lock().unwrap().push((fraction, stage));
        });

        sink(0.999, DownloadStage::Downloading);
        sink(1.0, DownloadStage::Verifying);

        assert_eq!(
            observed.lock().unwrap().clone(),
            vec![
                (0.999, DownloadStage::Downloading),
                (1.0, DownloadStage::Verifying),
            ]
        );
    }

    #[tokio::test]
    async fn download_model_rejects_unknown_diarization_variant_id() {
        let dir = tempfile::tempdir().unwrap();

        let err = download_model(dir.path(), "diarization-not-a-real-variant", |_, _| {})
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::ModelCatalogNotFound(_)));
    }
}
