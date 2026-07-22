//! Speaker diarization: sherpa-onnx model preparation (WP-5). Turn
//! production (WP-6) and the turn<->segment merge (WP-3/WP-7) build on top
//! of this module.

use crate::error::{AppError, Result};
use crate::models;
use crate::transcribe;
use std::collections::BTreeMap;
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

/// Overlap duration (ms) between two `[start, end)` spans, or 0 if disjoint.
/// Callers must pass well-formed spans (`start <= end`), matching the
/// invariant `Segment`/`SpeakerTurn` already uphold; a malformed span is a
/// caller bug, not a value this function tries to recover from.
fn overlap_ms(a: (u32, u32), b: (u32, u32)) -> u32 {
    debug_assert!(a.0 <= a.1 && b.0 <= b.1, "spans must be well-formed (start <= end)");
    let lo = a.0.max(b.0);
    let hi = a.1.min(b.1);
    hi.saturating_sub(lo)
}

/// Temporal gap (ms) between two `[start, end)` spans, or 0 if they touch or
/// overlap. Same well-formed-span precondition as `overlap_ms`.
fn gap_ms(a: (u32, u32), b: (u32, u32)) -> u32 {
    debug_assert!(a.0 <= a.1 && b.0 <= b.1, "spans must be well-formed (start <= end)");
    let lo = a.0.max(b.0);
    let hi = a.1.min(b.1);
    lo.saturating_sub(hi)
}

/// Assign each `segments` span the speaker whose diarization `turns`
/// maximally overlap it in time. Ties (including all-zero overlap) are
/// broken by the lowest speaker id. A segment with zero overlap against
/// every turn falls back to the temporally nearest turn's speaker, same
/// tie-break. An empty `turns` list yields `None` for every segment.
pub fn merge_segments_with_turns(
    segments: &[(u32, u32)],
    turns: &[SpeakerTurn],
) -> Vec<Option<i32>> {
    segments
        .iter()
        .map(|&segment| assign_speaker(segment, turns))
        .collect()
}

fn assign_speaker(segment: (u32, u32), turns: &[SpeakerTurn]) -> Option<i32> {
    if turns.is_empty() {
        return None;
    }

    let mut overlap_by_speaker: BTreeMap<i32, u32> = BTreeMap::new();
    for turn in turns {
        let overlap = overlap_ms(segment, (turn.start_ms, turn.end_ms));
        *overlap_by_speaker.entry(turn.speaker).or_insert(0) += overlap;
    }

    // BTreeMap iterates in ascending key order, so the first speaker to
    // reach the max overlap is the lowest id on any tie.
    let mut best: Option<(i32, u32)> = None;
    for (&speaker, &overlap) in &overlap_by_speaker {
        let is_new_best = match best {
            None => true,
            Some((_, best_overlap)) => overlap > best_overlap,
        };
        if is_new_best {
            best = Some((speaker, overlap));
        }
    }
    let (best_speaker, best_overlap) = best.expect("turns is non-empty, so overlap_by_speaker is too");

    if best_overlap > 0 {
        return Some(best_speaker);
    }

    // No turn overlaps this segment at all — fall back to the single
    // nearest turn (not per-speaker-aggregated), lowest speaker id on a tie.
    let mut nearest: Option<(u32, i32)> = None;
    for turn in turns {
        let gap = gap_ms(segment, (turn.start_ms, turn.end_ms));
        nearest = Some(match nearest {
            None => (gap, turn.speaker),
            Some((best_gap, best_speaker)) => {
                if gap < best_gap || (gap == best_gap && turn.speaker < best_speaker) {
                    (gap, turn.speaker)
                } else {
                    (best_gap, best_speaker)
                }
            }
        });
    }
    nearest.map(|(_, speaker)| speaker)
}

/// Assign `speaker_id` on each of `segments` per `turns`' overlap, in place.
/// A segment whose assignment comes back `None` (e.g. `turns` is empty) is
/// left untouched — this never regresses an already-set `speaker_id`, it
/// only ever fills one in.
pub fn assign_speaker_ids(segments: &mut [transcribe::Segment], turns: &[SpeakerTurn]) {
    let spans: Vec<(u32, u32)> = segments
        .iter()
        .map(|s| {
            (
                s.start_ms.min(u32::MAX as u64) as u32,
                s.end_ms.min(u32::MAX as u64) as u32,
            )
        })
        .collect();
    let assignments = merge_segments_with_turns(&spans, turns);

    for (segment, assignment) in segments.iter_mut().zip(assignments) {
        if let Some(speaker) = assignment {
            segment.speaker_id = Some(speaker);
        }
    }
}

/// Apply the outcome of a spawned diarization task to `segments`: on
/// success, assign speaker ids; on any failure — the diarization call
/// itself erroring, or the blocking task panicking/being cancelled — log a
/// warning and leave `segments` exactly as they were (speaker-less). A
/// diarization failure must never be treated as a transcription failure.
pub fn apply_diarization_outcome(
    segments: &mut [transcribe::Segment],
    outcome: std::result::Result<Result<Vec<SpeakerTurn>>, tokio::task::JoinError>,
) {
    match outcome {
        Ok(Ok(turns)) => assign_speaker_ids(segments, &turns),
        Ok(Err(e)) => log::warn!("diarization unavailable, returning speaker-less segments: {e}"),
        Err(e) => log::warn!("diarization task failed, returning speaker-less segments: {e}"),
    }
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

    fn speaker_turn(start_ms: u32, end_ms: u32, speaker: i32) -> SpeakerTurn {
        SpeakerTurn {
            start_ms,
            end_ms,
            speaker,
        }
    }

    // EP/BVA: full overlap, split overlap (tied and non-tied), gap fallback,
    // empty-turns, empty-segments, zero-duration span.

    #[test]
    fn merge_assigns_the_speaker_fully_covering_a_segment() {
        let segments = [(1_000, 2_000)];
        let turns = [speaker_turn(0, 3_000, 1)];

        assert_eq!(merge_segments_with_turns(&segments, &turns), vec![Some(1)]);
    }

    #[test]
    fn merge_breaks_a_tied_overlap_by_lowest_speaker_id() {
        let segments = [(0, 2_000)];
        // Speaker 2 listed first (1000ms overlap), speaker 1 second (1000ms
        // overlap) — equal, so speaker 1 must win regardless of list order.
        let turns = [speaker_turn(0, 1_000, 2), speaker_turn(1_000, 2_000, 1)];

        assert_eq!(merge_segments_with_turns(&segments, &turns), vec![Some(1)]);
    }

    #[test]
    fn merge_assigns_the_speaker_with_strictly_greater_overlap_even_with_a_higher_id() {
        let segments = [(0, 3_000)];
        // Speaker 5 has 2000ms overlap, speaker 3 has 1000ms — 5 must win on
        // overlap even though 3 < 5, proving tie-break isn't overriding a
        // real winner.
        let turns = [speaker_turn(0, 2_000, 5), speaker_turn(2_000, 3_000, 3)];

        assert_eq!(merge_segments_with_turns(&segments, &turns), vec![Some(5)]);
    }

    #[test]
    fn merge_falls_back_to_the_nearest_turn_in_a_gap_between_turns() {
        let turns = [speaker_turn(0, 1_000, 1), speaker_turn(3_000, 4_000, 2)];
        // Gap segment closer to turn 1 (500ms away) than turn 2 (1000ms away).
        let nearer_to_first = [(1_500, 2_000)];
        assert_eq!(
            merge_segments_with_turns(&nearer_to_first, &turns),
            vec![Some(1)]
        );

        // Gap segment after both turns, closer to the second (1000ms away
        // vs 4000ms away from the first).
        let after_both = [(5_000, 5_500)];
        assert_eq!(merge_segments_with_turns(&after_both, &turns), vec![Some(2)]);
    }

    #[test]
    fn merge_breaks_a_tied_gap_distance_by_lowest_speaker_id() {
        // Segment (2_000, 2_100) is exactly 1_000ms from both turns'
        // nearest edges — a genuine tie, not an approximation — and the
        // higher-id speaker is listed first, so this fails if the gap-side
        // tie-break clause is ever dropped or reversed.
        let turns = [speaker_turn(0, 1_000, 9), speaker_turn(3_100, 4_000, 3)];
        let segment = [(2_000, 2_100)];

        assert_eq!(merge_segments_with_turns(&segment, &turns), vec![Some(3)]);
    }

    #[test]
    fn merge_assigns_none_for_every_segment_when_turns_is_empty() {
        let segments = [(0, 1_000), (2_000, 3_000)];

        assert_eq!(
            merge_segments_with_turns(&segments, &[]),
            vec![None, None]
        );
    }

    #[test]
    fn merge_returns_empty_for_empty_segments() {
        let turns = [speaker_turn(0, 1_000, 1)];

        assert_eq!(merge_segments_with_turns(&[], &turns), Vec::<Option<i32>>::new());
    }

    #[test]
    fn merge_does_not_panic_on_a_zero_duration_segment() {
        let segments = [(1_000, 1_000)];
        let turns = [speaker_turn(0, 2_000, 1)];

        // A degenerate point span overlaps nothing (overlap is always 0 for
        // a zero-width span), so it falls back to the nearest turn — which
        // is turn 1 here (distance 0, since 1000 lies within [0, 2000)).
        assert_eq!(merge_segments_with_turns(&segments, &turns), vec![Some(1)]);
    }

    fn transcript_segment(start_ms: u64, end_ms: u64) -> transcribe::Segment {
        transcribe::Segment {
            start_ms,
            end_ms,
            text: "hello".to_string(),
            speaker_id: None,
        }
    }

    #[test]
    fn assign_speaker_ids_writes_the_merged_assignment_into_each_segment() {
        let mut segments = [
            transcript_segment(0, 1_000),
            transcript_segment(2_000, 3_000),
        ];
        let turns = [speaker_turn(0, 1_500, 7), speaker_turn(1_500, 3_500, 4)];

        assign_speaker_ids(&mut segments, &turns);

        assert_eq!(segments[0].speaker_id, Some(7));
        assert_eq!(segments[1].speaker_id, Some(4));
    }

    #[test]
    fn assign_speaker_ids_leaves_speaker_id_untouched_when_turns_is_empty() {
        let mut segments = [transcript_segment(0, 1_000), transcript_segment(2_000, 3_000)];

        assign_speaker_ids(&mut segments, &[]);

        assert_eq!(segments[0].speaker_id, None);
        assert_eq!(segments[1].speaker_id, None);
    }

    #[test]
    fn assign_speaker_ids_converts_u64_segment_spans_to_u32_correctly() {
        // Realistic multi-segment, non-zero timestamps well within u32
        // range, to exercise the u64 -> u32 conversion path directly
        // (rather than only ever testing with values that happen to fit
        // trivially, e.g. small round numbers already used above).
        let mut segments = [
            transcript_segment(61_234, 64_987),
            transcript_segment(70_000, 75_500),
        ];
        let turns = [
            speaker_turn(61_000, 65_000, 1),
            speaker_turn(65_000, 76_000, 2),
        ];

        assign_speaker_ids(&mut segments, &turns);

        assert_eq!(segments[0].speaker_id, Some(1));
        assert_eq!(segments[1].speaker_id, Some(2));
    }

    #[tokio::test]
    async fn apply_diarization_outcome_assigns_speakers_on_success() {
        let mut segments = [transcript_segment(0, 1_000)];
        let turns = vec![speaker_turn(0, 1_000, 3)];

        apply_diarization_outcome(&mut segments, Ok(Ok(turns)));

        assert_eq!(segments[0].speaker_id, Some(3));
    }

    #[tokio::test]
    async fn apply_diarization_outcome_leaves_segments_untouched_on_diarization_asset_error() {
        let mut segments = [transcript_segment(0, 1_000)];

        apply_diarization_outcome(
            &mut segments,
            Ok(Err(AppError::DiarizationAsset("models not downloaded".to_string()))),
        );

        assert_eq!(segments[0].speaker_id, None);
    }

    #[tokio::test]
    async fn apply_diarization_outcome_leaves_segments_untouched_on_engine_error() {
        let mut segments = [transcript_segment(0, 1_000)];

        apply_diarization_outcome(
            &mut segments,
            Ok(Err(AppError::Diarization("engine exploded".to_string()))),
        );

        assert_eq!(segments[0].speaker_id, None);
    }

    #[tokio::test]
    async fn apply_diarization_outcome_leaves_segments_untouched_when_the_task_panics() {
        let mut segments = [transcript_segment(0, 1_000)];

        let join_result = tokio::task::spawn_blocking(|| {
            panic!("simulated diarization task panic");
        })
        .await;
        let err = join_result.expect_err("spawned task was made to panic");

        apply_diarization_outcome(&mut segments, Err(err));

        assert_eq!(segments[0].speaker_id, None);
    }
}
