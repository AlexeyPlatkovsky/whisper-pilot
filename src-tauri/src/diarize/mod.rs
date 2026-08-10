//! Speaker diarization: sherpa-onnx model preparation (WP-5). Turn
//! production (WP-6) and the turn<->segment merge (WP-3/WP-7) build on top
//! of this module.
//!
//! The module is split by concern: [`clustering`] owns Rust-owned embedding
//! clustering, [`segmentation`] owns direct pyannote-style segmentation
//! inference, [`speakers`] owns turn merging and segment assignment, and
//! [`pipeline`] owns the Rust-owned end-to-end pipeline.

pub(crate) mod clustering;
pub(crate) mod pipeline;
pub(crate) mod segmentation;
pub(crate) mod speakers;

use crate::error::{AppError, Result};
use crate::models;
use std::io::Read;
use std::path::{Path, PathBuf};

pub use clustering::{cluster_embeddings, ClusterStop};
pub use segmentation::{powerset_class_to_activity, segmentation_windows};
pub use speakers::{apply_diarization_outcome, assign_speaker_ids, merge_segments_with_turns};
pub use speakers::SpeakerTurn;

use pipeline::diarize_with_rust_clustering;

const SEGMENTATION_ARCHIVE_ENTRY: &str = "model.onnx";
const AUTOMATIC_CLUSTER_THRESHOLD: f32 = 0.8;

/// Usable on-disk paths for direct segmentation and speaker-embedding
/// inference.
#[derive(Debug, Clone, PartialEq)]
pub struct DiarizationModelPaths {
    pub segmentation_model: PathBuf,
    pub embedding_model: PathBuf,
}

/// Reports direct segmentation and embedding work to the isolated worker's
/// inactivity supervisor. The return value is intentionally advisory: it
/// records liveness but does not cancel native inference.
pub type ProgressCallback = Box<dyn Fn(i32, i32) -> i32 + Send + 'static>;

/// Resolve the embedding model and segmentation model paths from the
/// diarization assets WP-39/WP-40 already downloaded into `app_support_dir`.
/// `active_variant` selects which downloaded embedding asset to use (e.g.
/// "campplus" or "titanet-large") — matched by id, not file extension, since
/// the diarization catalog entry now bundles more than one `.onnx` embedding
/// asset. The embedding model is used as-is; the shared segmentation model is
/// extracted (once) from its tar.bz2 archive into a stable, idempotent path.
pub fn resolve_diarization_models(
    app_support_dir: &Path,
    active_variant: &str,
) -> Result<DiarizationModelPaths> {
    let entry = models::CATALOG
        .iter()
        .find(|e| e.id == "diarization")
        .ok_or_else(|| {
            AppError::DiarizationAsset("diarization catalog entry not found".to_string())
        })?;
    let paths = models::asset_paths(app_support_dir, "diarization").ok_or_else(|| {
        AppError::DiarizationAsset("diarization catalog entry not found".to_string())
    })?;

    let mut archive_path = None;
    let mut embedding_path = None;
    for (asset, path) in entry.assets.iter().zip(paths.iter()) {
        match asset.variant_id {
            None => archive_path = Some(path),
            Some(variant_id) if variant_id == active_variant => embedding_path = Some(path),
            _ => {}
        }
    }

    let archive_path = archive_path.ok_or_else(|| {
        AppError::DiarizationAsset(
            "diarization catalog entry is missing its shared segmentation archive asset"
                .to_string(),
        )
    })?;
    if archive_path.extension().and_then(|e| e.to_str()) != Some("bz2") {
        return Err(AppError::DiarizationAsset(
            "diarization catalog entry's shared asset is not a segmentation archive (.tar.bz2)"
                .to_string(),
        ));
    }
    let embedding_model = embedding_path.ok_or_else(|| {
        AppError::DiarizationAsset(format!(
            "diarization catalog entry has no embedding asset for variant '{active_variant}'"
        ))
    })?;

    let segmentation_model = extract_segmentation_model(archive_path)?;

    Ok(DiarizationModelPaths {
        segmentation_model,
        embedding_model: embedding_model.clone(),
    })
}

/// Extract `model.onnx` from the segmentation archive into a stable sibling
/// directory next to the archive, returning its path. Idempotent: if the
/// target already exists, the archive is not re-read. Safe across
/// concurrent *processes* (the temp file name is process-unique); callers
/// within the same process must still serialize calls to avoid a
/// check-then-write race on the target path — no such concurrent caller
/// exists yet (WP-6 runs diarization once per Transcribe run).
fn extract_segmentation_model(archive_path: &Path) -> Result<PathBuf> {
    let target_dir = archive_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("sherpa-onnx-pyannote-segmentation-3-0");
    let target_path = target_dir.join(SEGMENTATION_ARCHIVE_ENTRY);

    if target_path.exists() {
        return Ok(target_path);
    }

    let file = std::fs::File::open(archive_path).map_err(|e| {
        AppError::DiarizationAsset(format!(
            "could not open segmentation archive at {}: {e}",
            archive_path.display()
        ))
    })?;
    let decoder = bzip2::read::BzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    let entries = archive
        .entries()
        .map_err(|e| AppError::DiarizationAsset(format!("corrupt segmentation archive: {e}")))?;

    let mut found = None;
    for entry in entries {
        let mut entry = entry.map_err(|e| {
            AppError::DiarizationAsset(format!("corrupt segmentation archive: {e}"))
        })?;
        let path = entry.path().map_err(|e| {
            AppError::DiarizationAsset(format!("corrupt segmentation archive entry path: {e}"))
        })?;
        if path.file_name().and_then(|n| n.to_str()) == Some(SEGMENTATION_ARCHIVE_ENTRY) {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| {
                AppError::DiarizationAsset(format!("corrupt segmentation archive: {e}"))
            })?;
            found = Some(bytes);
            break;
        }
    }

    let bytes = found.ok_or_else(|| {
        AppError::DiarizationAsset(format!(
            "segmentation archive at {} has no {} entry",
            archive_path.display(),
            SEGMENTATION_ARCHIVE_ENTRY
        ))
    })?;

    std::fs::create_dir_all(&target_dir)?;
    let temp_path = target_dir.join(format!(
        "{SEGMENTATION_ARCHIVE_ENTRY}.{}.part",
        std::process::id()
    ));
    std::fs::write(&temp_path, &bytes)?;
    std::fs::rename(&temp_path, &target_path)?;

    Ok(target_path)
}

/// Run speaker diarization over `samples` (16kHz mono f32), using the models
/// WP-5's `resolve_diarization_models` prepares. Automatic threshold
/// clustering only; positive `speaker_count` is rejected until WP-49 owns
/// fixed-count behavior.
///
/// Runs native inference in this process — `transcribe_meeting` instead goes
/// through `diarize_process::diarize_isolated`. Retained as the no-progress
/// form used by `tests/diarize_integration.rs`.
pub fn diarize_samples(
    app_support_dir: &Path,
    samples: Vec<f32>,
    speaker_count: Option<i32>,
    active_variant: &str,
) -> Result<Vec<SpeakerTurn>> {
    diarize_samples_with_progress(
        app_support_dir,
        samples,
        speaker_count,
        active_variant,
        None,
    )
}

/// [`diarize_samples`] with a liveness callback. The isolating child passes one
/// so its supervising parent can tell a working engine from a hung one; see
/// `diarize_process`.
pub fn diarize_samples_with_progress(
    app_support_dir: &Path,
    samples: Vec<f32>,
    speaker_count: Option<i32>,
    active_variant: &str,
    progress: Option<ProgressCallback>,
) -> Result<Vec<SpeakerTurn>> {
    let models = resolve_diarization_models(app_support_dir, active_variant)?;
    diarize_with_rust_clustering(
        &models,
        samples,
        speaker_count,
        AUTOMATIC_CLUSTER_THRESHOLD,
        progress,
    )
}

/// Route-A measurement seam: run the Rust-owned path at a supplied
/// cosine-distance threshold without changing the production default.
pub fn diarize_samples_with_cluster_threshold(
    app_support_dir: &Path,
    samples: Vec<f32>,
    active_variant: &str,
    threshold: f32,
) -> Result<Vec<SpeakerTurn>> {
    let models = resolve_diarization_models(app_support_dir, active_variant)?;
    diarize_with_rust_clustering(&models, samples, None, threshold, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    /// Isolates the real sherpa-onnx engine call so the surrounding logic
    /// (config translation, ordering) is unit-testable without real ONNX
    /// inference.
    trait SpeakerDiarizer {
        fn compute(&mut self, samples: Vec<f32>) -> Result<Vec<SpeakerTurn>>;
    }

    /// Tuned empirically against real recordings — see docs/architecture.md's
    /// Speaker Diarization section and WP-50's TaskPilot comments.
    ///
    /// Quality-optimal, *not* crash-safe: it sits above the native crash boundary
    /// for the recommended embedding model, and `diarize_process`'s isolation is
    /// what makes it affordable (ADR-013). Do not remove that isolation on the
    /// grounds that this value works.
    const AUTO_DETECT_THRESHOLD: f32 = 0.9;
    const AUTO_DETECT_MIN_DURATION_ON: f32 = 1.0;
    const AUTO_DETECT_MIN_DURATION_OFF: f32 = 1.0;

    /// Translate a caller-provided speaker count into sherpa-onnx's clustering
    /// config. `num_clusters < 1` means "auto-detect via the threshold instead";
    /// the crate's own default is a fixed `num_clusters: Some(4)`.
    ///
    /// `min_duration_on`/`min_duration_off` are gated to the auto-detect branch
    /// only, since setting them unconditionally would silently affect WP-49's
    /// not-yet-built explicit-count path. See docs/architecture.md's Speaker
    /// Diarization section.
    fn build_config(speaker_count: Option<i32>) -> sherpa_rs::diarize::DiarizeConfig {
        let is_auto_detect = speaker_count.filter(|&n| n > 0).is_none();
        let num_clusters = speaker_count.filter(|&n| n > 0).unwrap_or(0);
        sherpa_rs::diarize::DiarizeConfig {
            num_clusters: Some(num_clusters),
            threshold: Some(AUTO_DETECT_THRESHOLD),
            min_duration_on: Some(if is_auto_detect {
                AUTO_DETECT_MIN_DURATION_ON
            } else {
                0.0
            }),
            min_duration_off: Some(if is_auto_detect {
                AUTO_DETECT_MIN_DURATION_OFF
            } else {
                0.0
            }),
            ..Default::default()
        }
    }

    /// Run `diarizer` over `samples` and return its turns ordered by start time.
    /// Defensive: sherpa-onnx already returns turns start-time-sorted, but this
    /// does not depend on that undocumented behavior.
    fn diarize_with(
        diarizer: &mut impl SpeakerDiarizer,
        samples: Vec<f32>,
    ) -> Result<Vec<SpeakerTurn>> {
        let mut turns = diarizer.compute(samples)?;
        turns.sort_by_key(|t| t.start_ms);
        Ok(turns)
    }

    /// Build a real tar.bz2 archive at `archive_path`, containing a
    /// `sherpa-onnx-pyannote-segmentation-3-0/model.onnx` entry (and, to
    /// mirror the real release archive, a sibling `model.int8.onnx` entry
    /// that must NOT be the one extracted) with the given bytes.
    fn build_fixture_archive(archive_path: &Path, model_onnx_bytes: &[u8]) {
        let file = std::fs::File::create(archive_path).unwrap();
        let encoder = bzip2::write::BzEncoder::new(file, bzip2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);

        let mut append = |name: &str, bytes: &[u8]| {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, name, bytes)
                .expect("append fixture entry");
        };
        append(
            "sherpa-onnx-pyannote-segmentation-3-0/model.onnx",
            model_onnx_bytes,
        );
        append(
            "sherpa-onnx-pyannote-segmentation-3-0/model.int8.onnx",
            b"not the model we want",
        );
        append(
            "sherpa-onnx-pyannote-segmentation-3-0/README.md",
            b"fixture readme",
        );

        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();
    }

    /// Writes fixture bytes for the diarization entry's shared segmentation
    /// archive plus BOTH embedding variants (paths[1] = campplus, paths[2] =
    /// titanet-large per the restructured 3-asset entry), returning the
    /// requested variant's expected on-disk bytes.
    fn write_embedding_fixtures(dir: &Path) -> (Vec<u8>, Vec<u8>) {
        let paths = models::asset_paths(dir, "diarization").unwrap();
        let campplus_bytes = b"fake campplus embedding model bytes".to_vec();
        let titanet_bytes = b"fake titanet-large embedding model bytes".to_vec();
        std::fs::create_dir_all(paths[1].parent().unwrap()).unwrap();
        std::fs::write(&paths[1], &campplus_bytes).unwrap();
        std::fs::write(&paths[2], &titanet_bytes).unwrap();
        (campplus_bytes, titanet_bytes)
    }

    #[test]
    fn resolve_diarization_models_extracts_segmentation_and_returns_the_active_variants_embedding_as_is(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let paths = models::asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        let model_bytes = b"the real segmentation model bytes";
        build_fixture_archive(&paths[0], model_bytes);
        let (campplus_bytes, titanet_bytes) = write_embedding_fixtures(dir.path());

        let resolved_campplus = resolve_diarization_models(dir.path(), "campplus").unwrap();
        let resolved_titanet = resolve_diarization_models(dir.path(), "titanet-large").unwrap();

        assert_eq!(resolved_campplus.embedding_model, paths[1]);
        assert_eq!(
            std::fs::read(&resolved_campplus.embedding_model).unwrap(),
            campplus_bytes
        );
        assert_eq!(resolved_titanet.embedding_model, paths[2]);
        assert_eq!(
            std::fs::read(&resolved_titanet.embedding_model).unwrap(),
            titanet_bytes
        );
        for resolved in [&resolved_campplus, &resolved_titanet] {
            let extracted = std::fs::read(&resolved.segmentation_model).unwrap();
            assert_eq!(extracted, model_bytes);
            assert!(resolved
                .segmentation_model
                .to_string_lossy()
                .ends_with("model.onnx"));
            assert!(!resolved
                .segmentation_model
                .to_string_lossy()
                .ends_with("model.int8.onnx"));
        }
    }

    #[test]
    fn resolve_diarization_models_fails_closed_for_an_unknown_active_variant() {
        let dir = tempfile::tempdir().unwrap();
        let paths = models::asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        build_fixture_archive(&paths[0], b"segmentation model bytes");
        write_embedding_fixtures(dir.path());

        let err = resolve_diarization_models(dir.path(), "not-a-real-variant").unwrap_err();

        assert!(matches!(err, AppError::DiarizationAsset(_)));
    }

    #[test]
    fn resolve_diarization_models_is_idempotent_on_second_call() {
        let dir = tempfile::tempdir().unwrap();
        let paths = models::asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        build_fixture_archive(&paths[0], b"segmentation model v1");
        write_embedding_fixtures(dir.path());

        let first = resolve_diarization_models(dir.path(), "campplus").unwrap();
        // Overwrite the archive with different bytes; a correctly idempotent
        // implementation must not re-read it, so the previously extracted
        // file's content must be unchanged.
        build_fixture_archive(&paths[0], b"segmentation model v2 (should be ignored)");

        let second = resolve_diarization_models(dir.path(), "campplus").unwrap();

        assert_eq!(first.segmentation_model, second.segmentation_model);
        let content = std::fs::read(&second.segmentation_model).unwrap();
        assert_eq!(content, b"segmentation model v1");
    }

    #[test]
    fn resolve_diarization_models_fails_closed_when_archive_missing() {
        let dir = tempfile::tempdir().unwrap();
        write_embedding_fixtures(dir.path());
        // No archive written at paths[0].

        let err = resolve_diarization_models(dir.path(), "campplus").unwrap_err();

        assert!(matches!(err, AppError::DiarizationAsset(_)));
    }

    #[test]
    fn resolve_diarization_models_fails_closed_on_corrupt_archive_without_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = models::asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        std::fs::write(&paths[0], b"not a real bzip2 tar archive at all").unwrap();
        write_embedding_fixtures(dir.path());

        let err = resolve_diarization_models(dir.path(), "campplus").unwrap_err();

        assert!(matches!(err, AppError::DiarizationAsset(_)));
        let target = paths[0]
            .parent()
            .unwrap()
            .join("sherpa-onnx-pyannote-segmentation-3-0")
            .join("model.onnx");
        assert!(
            !target.exists(),
            "no partial file left after a failed extraction"
        );
    }

    #[test]
    fn extract_segmentation_model_rejects_archive_missing_the_expected_entry() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("segmentation.tar.bz2");
        let file = std::fs::File::create(&archive_path).unwrap();
        let encoder = bzip2::write::BzEncoder::new(file, bzip2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "sherpa-onnx-pyannote-segmentation-3-0/README.md",
                &b"nope"[..],
            )
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        let err = extract_segmentation_model(&archive_path).unwrap_err();

        assert!(matches!(err, AppError::DiarizationAsset(_)));
    }

    #[test]
    fn diarize_samples_propagates_asset_error_when_models_not_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        // No diarization assets written under `dir` at all.

        let err = diarize_samples(dir.path(), vec![0.0; 16_000], None, "campplus").unwrap_err();

        assert!(matches!(err, AppError::DiarizationAsset(_)));
    }

    #[test]
    fn diarize_samples_propagates_asset_error_for_an_unknown_active_variant() {
        let dir = tempfile::tempdir().unwrap();
        let paths = models::asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        build_fixture_archive(&paths[0], b"segmentation model bytes");
        write_embedding_fixtures(dir.path());

        let err =
            diarize_samples(dir.path(), vec![0.0; 16_000], None, "not-a-real-variant").unwrap_err();

        assert!(matches!(err, AppError::DiarizationAsset(_)));
    }

    struct FakeDiarizer {
        result: Option<Result<Vec<SpeakerTurn>>>,
    }

    impl SpeakerDiarizer for FakeDiarizer {
        fn compute(&mut self, _samples: Vec<f32>) -> Result<Vec<SpeakerTurn>> {
            self.result.take().expect("compute called more than once")
        }
    }

    fn turn(start_ms: u32, end_ms: u32, speaker: i32) -> SpeakerTurn {
        SpeakerTurn {
            start_ms,
            end_ms,
            speaker,
        }
    }

    // EP: two classes — "auto-detect" (None, and non-positive counts) vs.
    // "explicit count" (positive n). BVA: the boundary sits between 0 and 1.
    #[test]
    fn build_config_auto_detects_when_no_count_given() {
        assert_eq!(build_config(None).num_clusters, Some(0));
    }

    #[test]
    fn build_config_uses_a_provided_positive_count() {
        assert_eq!(build_config(Some(3)).num_clusters, Some(3));
    }

    #[test]
    fn build_config_uses_the_smallest_positive_count_at_the_auto_detect_boundary() {
        assert_eq!(build_config(Some(1)).num_clusters, Some(1));
    }

    #[test]
    fn build_config_treats_non_positive_counts_as_auto_detect() {
        assert_eq!(build_config(Some(0)).num_clusters, Some(0));
        assert_eq!(build_config(Some(-1)).num_clusters, Some(0));
    }

    #[test]
    fn build_config_auto_detect_sets_the_tuned_threshold_and_min_durations() {
        let config = build_config(None);
        assert_eq!(config.threshold, Some(0.9));
        assert_eq!(config.min_duration_on, Some(1.0));
        assert_eq!(config.min_duration_off, Some(1.0));
    }

    // See build_config's doc comment for why min_duration (unlike threshold)
    // must not carry over to the explicit-count path.
    #[test]
    fn build_config_sets_the_tuned_threshold_but_crate_default_min_durations_for_an_explicit_count()
    {
        let config = build_config(Some(2));
        assert_eq!(config.threshold, Some(0.9));
        assert_eq!(config.min_duration_on, Some(0.0));
        assert_eq!(config.min_duration_off, Some(0.0));
    }

    #[test]
    fn diarize_with_sorts_turns_by_start_time() {
        let mut fake = FakeDiarizer {
            result: Some(Ok(vec![turn(5_000, 6_000, 2), turn(0, 4_000, 1)])),
        };

        let turns = diarize_with(&mut fake, vec![]).unwrap();

        assert_eq!(turns, vec![turn(0, 4_000, 1), turn(5_000, 6_000, 2)]);
    }

    #[test]
    fn diarize_with_propagates_engine_error_without_panicking() {
        let mut fake = FakeDiarizer {
            result: Some(Err(AppError::Diarization("engine exploded".to_string()))),
        };

        let err = diarize_with(&mut fake, vec![]).unwrap_err();

        assert!(matches!(err, AppError::Diarization(_)));
    }
}
