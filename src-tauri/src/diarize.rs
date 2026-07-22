//! Speaker diarization: sherpa-onnx model preparation (WP-5). Turn
//! production (WP-6) and the turn<->segment merge (WP-3/WP-7) build on top
//! of this module.

use crate::error::{AppError, Result};
use crate::models;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Usable on-disk paths for the two diarization models, ready to hand to
/// sherpa-onnx's `Diarize::new`.
#[derive(Debug, Clone, PartialEq)]
pub struct DiarizationModelPaths {
    pub segmentation_model: PathBuf,
    pub embedding_model: PathBuf,
}

const SEGMENTATION_ARCHIVE_ENTRY: &str = "model.onnx";

/// Resolve the embedding model and segmentation model paths from the
/// diarization assets WP-39/WP-40 already downloaded into `app_support_dir`.
/// The embedding model is used as-is; the segmentation model is extracted
/// (once) from its tar.bz2 archive into a stable, idempotent path.
pub fn resolve_diarization_models(app_support_dir: &Path) -> Result<DiarizationModelPaths> {
    let paths = models::asset_paths(app_support_dir, "diarization").ok_or_else(|| {
        AppError::DiarizationAsset("diarization catalog entry not found".to_string())
    })?;

    // Match by extension rather than catalog position, so a future reordering
    // of the "diarization" entry's assets can't silently swap which file is
    // treated as the archive vs. the plain embedding model.
    let has_ext = |p: &Path, ext: &str| p.extension().and_then(|e| e.to_str()) == Some(ext);
    let archive_path = paths.iter().find(|p| has_ext(p, "bz2")).ok_or_else(|| {
        AppError::DiarizationAsset(
            "diarization catalog entry is missing its segmentation archive (.tar.bz2) asset"
                .to_string(),
        )
    })?;
    let embedding_model = paths.iter().find(|p| has_ext(p, "onnx")).ok_or_else(|| {
        AppError::DiarizationAsset(
            "diarization catalog entry is missing its embedding (.onnx) asset".to_string(),
        )
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
        let mut entry = entry
            .map_err(|e| AppError::DiarizationAsset(format!("corrupt segmentation archive: {e}")))?;
        let path = entry.path().map_err(|e| {
            AppError::DiarizationAsset(format!("corrupt segmentation archive entry path: {e}"))
        })?;
        if path.file_name().and_then(|n| n.to_str()) == Some(SEGMENTATION_ARCHIVE_ENTRY) {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|e| AppError::DiarizationAsset(format!("corrupt segmentation archive: {e}")))?;
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

/// One speaker's contiguous span of speech, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeakerTurn {
    pub start_ms: u32,
    pub end_ms: u32,
    pub speaker: i32,
}

/// Isolates the real sherpa-onnx engine call so the surrounding logic
/// (config translation, ordering) is unit-testable without real ONNX
/// inference.
trait SpeakerDiarizer {
    fn compute(&mut self, samples: Vec<f32>) -> Result<Vec<SpeakerTurn>>;
}

/// Translate a caller-provided speaker count into sherpa-onnx's clustering
/// config. `num_clusters < 1` means "auto-detect via the threshold instead"
/// (verified against sherpa-onnx's own `fast-clustering-config.cc`) — the
/// crate's own `DiarizeConfig::default()` sets `num_clusters: Some(4)`, which
/// is a fixed count, not auto-detect, so this must be set explicitly.
fn build_config(speaker_count: Option<i32>) -> sherpa_rs::diarize::DiarizeConfig {
    let num_clusters = speaker_count.filter(|&n| n > 0).unwrap_or(0);
    sherpa_rs::diarize::DiarizeConfig {
        num_clusters: Some(num_clusters),
        ..Default::default()
    }
}

/// Run `diarizer` over `samples` and return its turns ordered by start time.
/// Defensive: sherpa-onnx already returns turns start-time-sorted, but this
/// does not depend on that undocumented behavior.
fn diarize_with(diarizer: &mut impl SpeakerDiarizer, samples: Vec<f32>) -> Result<Vec<SpeakerTurn>> {
    let mut turns = diarizer.compute(samples)?;
    turns.sort_by_key(|t| t.start_ms);
    Ok(turns)
}

/// Adapts the real `sherpa_rs::diarize::Diarize` engine to `SpeakerDiarizer`,
/// converting its seconds-based `f32` output to our milliseconds-based `u32`
/// `SpeakerTurn`.
struct SherpaDiarizer(sherpa_rs::diarize::Diarize);

impl SpeakerDiarizer for SherpaDiarizer {
    fn compute(&mut self, samples: Vec<f32>) -> Result<Vec<SpeakerTurn>> {
        let segments = self
            .0
            .compute(samples, None)
            .map_err(|e| AppError::Diarization(e.to_string()))?;
        Ok(segments
            .into_iter()
            .map(|s| SpeakerTurn {
                start_ms: (s.start * 1000.0).round() as u32,
                end_ms: (s.end * 1000.0).round() as u32,
                speaker: s.speaker,
            })
            .collect())
    }
}

/// Run speaker diarization over `samples` (16kHz mono f32), using the models
/// WP-5's `resolve_diarization_models` prepares. `speaker_count`: `Some(n)`
/// with `n > 0` requests exactly `n` speakers; anything else auto-detects.
pub fn diarize_samples(
    app_support_dir: &Path,
    samples: Vec<f32>,
    speaker_count: Option<i32>,
) -> Result<Vec<SpeakerTurn>> {
    let models = resolve_diarization_models(app_support_dir)?;
    let config = build_config(speaker_count);
    let engine = sherpa_rs::diarize::Diarize::new(
        models.segmentation_model,
        models.embedding_model,
        config,
    )
    .map_err(|e| AppError::Diarization(format!("failed to initialize diarizer: {e}")))?;
    let mut adapter = SherpaDiarizer(engine);
    diarize_with(&mut adapter, samples)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn write_embedding_fixture(dir: &Path) -> Vec<u8> {
        let paths = models::asset_paths(dir, "diarization").unwrap();
        let embedding_bytes = b"fake embedding model bytes".to_vec();
        std::fs::create_dir_all(paths[1].parent().unwrap()).unwrap();
        std::fs::write(&paths[1], &embedding_bytes).unwrap();
        embedding_bytes
    }

    #[test]
    fn resolve_diarization_models_extracts_segmentation_and_returns_embedding_as_is() {
        let dir = tempfile::tempdir().unwrap();
        let paths = models::asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        let model_bytes = b"the real segmentation model bytes";
        build_fixture_archive(&paths[0], model_bytes);
        let embedding_bytes = write_embedding_fixture(dir.path());

        let resolved = resolve_diarization_models(dir.path()).unwrap();

        assert_eq!(resolved.embedding_model, paths[1]);
        assert_eq!(std::fs::read(&resolved.embedding_model).unwrap(), embedding_bytes);
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

    #[test]
    fn resolve_diarization_models_is_idempotent_on_second_call() {
        let dir = tempfile::tempdir().unwrap();
        let paths = models::asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        build_fixture_archive(&paths[0], b"segmentation model v1");
        write_embedding_fixture(dir.path());

        let first = resolve_diarization_models(dir.path()).unwrap();
        // Overwrite the archive with different bytes; a correctly idempotent
        // implementation must not re-read it, so the previously extracted
        // file's content must be unchanged.
        build_fixture_archive(&paths[0], b"segmentation model v2 (should be ignored)");

        let second = resolve_diarization_models(dir.path()).unwrap();

        assert_eq!(first.segmentation_model, second.segmentation_model);
        let content = std::fs::read(&second.segmentation_model).unwrap();
        assert_eq!(content, b"segmentation model v1");
    }

    #[test]
    fn resolve_diarization_models_fails_closed_when_archive_missing() {
        let dir = tempfile::tempdir().unwrap();
        write_embedding_fixture(dir.path());
        // No archive written at paths[0].

        let err = resolve_diarization_models(dir.path()).unwrap_err();

        assert!(matches!(err, AppError::DiarizationAsset(_)));
    }

    #[test]
    fn resolve_diarization_models_fails_closed_on_corrupt_archive_without_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = models::asset_paths(dir.path(), "diarization").unwrap();
        std::fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        std::fs::write(&paths[0], b"not a real bzip2 tar archive at all").unwrap();
        write_embedding_fixture(dir.path());

        let err = resolve_diarization_models(dir.path()).unwrap_err();

        assert!(matches!(err, AppError::DiarizationAsset(_)));
        let target = paths[0]
            .parent()
            .unwrap()
            .join("sherpa-onnx-pyannote-segmentation-3-0")
            .join("model.onnx");
        assert!(!target.exists(), "no partial file left after a failed extraction");
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
            .append_data(&mut header, "sherpa-onnx-pyannote-segmentation-3-0/README.md", &b"nope"[..])
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        let err = extract_segmentation_model(&archive_path).unwrap_err();

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

    #[test]
    fn diarize_samples_propagates_asset_error_when_models_not_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        // No diarization assets written under `dir` at all.

        let err = diarize_samples(dir.path(), vec![0.0; 16_000], None).unwrap_err();

        assert!(matches!(err, AppError::DiarizationAsset(_)));
    }
}
