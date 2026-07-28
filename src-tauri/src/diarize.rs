//! Speaker diarization: sherpa-onnx model preparation (WP-5). Turn
//! production (WP-6) and the turn<->segment merge (WP-3/WP-7) build on top
//! of this module.

use crate::error::{AppError, Result};
use crate::models;
use crate::transcribe;
use ndarray::{s, Array3, CowArray};
use ort::{
    tensor::OrtOwnedTensor, Environment, GraphOptimizationLevel, Session, SessionBuilder, Value,
};
use sherpa_rs::speaker_id::{EmbeddingExtractor, ExtractorConfig};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Usable on-disk paths for direct segmentation and speaker-embedding
/// inference.
#[derive(Debug, Clone, PartialEq)]
pub struct DiarizationModelPaths {
    pub segmentation_model: PathBuf,
    pub embedding_model: PathBuf,
}

const SEGMENTATION_ARCHIVE_ENTRY: &str = "model.onnx";
const SEGMENTATION_BATCH_SIZE: usize = 32;
// pyannote segmentation 3.0 advances every inference window by ten percent.
// This is distinct from `receptive_field_shift`, which timestamps output frames.
const SEGMENTATION_WINDOW_SHIFT_DIVISOR: usize = 10;
const MIN_EMBEDDING_FRAMES: usize = 10;
const MIN_TURN_DURATION_MS: u32 = 300;
const MAX_SPEAKER_GAP_MS: u32 = 500;
const AUTOMATIC_CLUSTER_THRESHOLD: f32 = 0.8;
const ORT_DYLIB_NAME: &str = "libonnxruntime.1.17.1.dylib";
static ORT_DYLIB_PATH: OnceLock<PathBuf> = OnceLock::new();
static ORT_ENVIRONMENT: OnceLock<Arc<Environment>> = OnceLock::new();
static ORT_ENVIRONMENT_INIT: Mutex<()> = Mutex::new(());

/// The stopping rule for Rust-owned speaker clustering.
///
/// WP-62 ships the distance-threshold mode. The fixed-count variant remains
/// represented here so WP-49 can add its speaker-count override without
/// redesigning the public clustering contract.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClusterStop {
    /// Stop merging once the nearest pair is farther apart than this cosine
    /// distance. Valid values are in the inclusive cosine-distance range 0–2.
    Distance(f32),
    /// Reserved for WP-49; deliberately not implemented in WP-62.
    FixedCount(usize),
}

#[derive(Debug)]
struct EmbeddingCluster {
    members: Vec<usize>,
    centroid: Vec<f32>,
}

/// Deterministic incremental-centroid threshold clustering for normalized
/// embeddings. Labels follow first input occurrence; see Speaker Diarization
/// in `docs/architecture.md` for the production-path constraint.
pub fn cluster_embeddings(embeddings: &[Vec<f32>], stop: ClusterStop) -> Result<Vec<usize>> {
    let threshold = match stop {
        ClusterStop::Distance(value) if value.is_finite() && (0.0..=2.0).contains(&value) => value,
        ClusterStop::Distance(value) => {
            return Err(AppError::Diarization(format!(
                "invalid cosine-distance threshold {value}; expected a finite value from 0 to 2"
            )));
        }
        ClusterStop::FixedCount(_) => {
            return Err(AppError::Diarization(
                "fixed-count speaker clustering is reserved for WP-49".to_string(),
            ));
        }
    };

    if embeddings.is_empty() {
        return Ok(Vec::new());
    }

    let dimension = embeddings[0].len();
    if dimension == 0 {
        return Err(AppError::Diarization(
            "speaker embedding must have at least one dimension".to_string(),
        ));
    }

    let mut clusters: Vec<EmbeddingCluster> = Vec::new();
    for (index, embedding) in embeddings.iter().enumerate() {
        if embedding.len() != dimension {
            return Err(AppError::Diarization(format!(
                "speaker embedding {index} has dimension {}; expected {dimension}",
                embedding.len()
            )));
        }
        let normalized = normalize_embedding(embedding, index)?;
        let nearest = clusters
            .iter()
            .enumerate()
            .map(|(cluster_index, cluster)| {
                (
                    cluster_index,
                    cosine_distance(&normalized, &cluster.centroid),
                )
            })
            .min_by(|left, right| left.1.total_cmp(&right.1));

        if let Some((cluster_index, distance)) = nearest {
            if distance <= threshold {
                let cluster = &mut clusters[cluster_index];
                cluster.centroid = weighted_normalized_centroid(
                    &cluster.centroid,
                    cluster.members.len(),
                    &normalized,
                )?;
                cluster.members.push(index);
                continue;
            }
        }

        clusters.push(EmbeddingCluster {
            members: vec![index],
            centroid: normalized,
        });
    }

    let mut labels = vec![0; embeddings.len()];
    for (label, cluster) in clusters.iter().enumerate() {
        for &member in &cluster.members {
            labels[member] = label;
        }
    }
    Ok(labels)
}

fn normalize_embedding(embedding: &[f32], index: usize) -> Result<Vec<f32>> {
    if embedding.iter().any(|value| !value.is_finite()) {
        return Err(AppError::Diarization(format!(
            "speaker embedding {index} contains a non-finite value"
        )));
    }
    let norm_squared: f32 = embedding.iter().map(|value| value * value).sum();
    if !norm_squared.is_finite() || norm_squared <= f32::EPSILON {
        return Err(AppError::Diarization(format!(
            "speaker embedding {index} has zero magnitude"
        )));
    }
    let norm = norm_squared.sqrt();
    Ok(embedding.iter().map(|value| value / norm).collect())
}

fn weighted_normalized_centroid(
    centroid: &[f32],
    centroid_members: usize,
    embedding: &[f32],
) -> Result<Vec<f32>> {
    let member_count = centroid_members as f32;
    let mean: Vec<f32> = centroid
        .iter()
        .zip(embedding)
        .map(|(centroid_value, embedding_value)| {
            (centroid_value * member_count + embedding_value) / (member_count + 1.0)
        })
        .collect();
    normalize_embedding(&mean, 0).map_err(|_| {
        AppError::Diarization("cannot merge speaker clusters with opposite centroids".to_string())
    })
}

fn cosine_distance(left: &[f32], right: &[f32]) -> f32 {
    let similarity: f32 = left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum();
    1.0 - similarity.clamp(-1.0, 1.0)
}

/// Expand one pyannote powerset class into activity for its local speakers.
///
/// Class zero is silence; classes then enumerate singleton combinations before
/// two-speaker combinations in lexicographic order. The downloaded
/// segmentation model advertises the dimensions used here as ONNX metadata.
pub fn powerset_class_to_activity(
    class: usize,
    num_speakers: usize,
    powerset_max_classes: usize,
) -> Result<Vec<bool>> {
    if num_speakers == 0 {
        return Err(AppError::Diarization(
            "segmentation metadata declares zero local speakers".to_string(),
        ));
    }
    if powerset_max_classes == 0 || powerset_max_classes > 2 {
        return Err(AppError::Diarization(format!(
            "unsupported pyannote powerset size {powerset_max_classes}; expected 1 or 2"
        )));
    }

    if class == 0 {
        return Ok(vec![false; num_speakers]);
    }

    let mut remaining = class - 1;
    if remaining < num_speakers {
        let mut activity = vec![false; num_speakers];
        activity[remaining] = true;
        return Ok(activity);
    }
    remaining -= num_speakers;

    if powerset_max_classes == 2 {
        for left in 0..num_speakers {
            for right in left + 1..num_speakers {
                if remaining == 0 {
                    let mut activity = vec![false; num_speakers];
                    activity[left] = true;
                    activity[right] = true;
                    return Ok(activity);
                }
                remaining -= 1;
            }
        }
    }

    Err(AppError::Diarization(format!(
        "segmentation output class {class} is outside the declared powerset"
    )))
}

/// Split mono samples into pyannote's fixed-size, overlapping inference
/// windows. The final partial window is retained with zero padding so the end
/// of the recording cannot silently lose speaker activity.
pub fn segmentation_windows(
    samples: &[f32],
    window_size: usize,
    window_shift: usize,
) -> Result<Vec<(usize, Vec<f32>)>> {
    if window_size == 0 || window_shift == 0 {
        return Err(AppError::Diarization(
            "segmentation metadata has a zero window size or shift".to_string(),
        ));
    }
    if samples.is_empty() {
        return Ok(Vec::new());
    }

    let mut starts = Vec::new();
    if samples.len() <= window_size {
        starts.push(0);
    } else {
        let full_count = (samples.len() - window_size) / window_shift + 1;
        starts.extend((0..full_count).map(|index| index * window_shift));
        if (samples.len() - window_size) % window_shift != 0 {
            starts.push(full_count * window_shift);
        }
    }

    Ok(starts
        .into_iter()
        .map(|start| {
            let mut window = vec![0.0; window_size];
            let available = samples.len().saturating_sub(start).min(window_size);
            window[..available].copy_from_slice(&samples[start..start + available]);
            (start, window)
        })
        .collect())
}

fn segmentation_window_shift(window_size: usize) -> Result<usize> {
    let window_shift = window_size / SEGMENTATION_WINDOW_SHIFT_DIVISOR;
    if window_shift == 0 {
        return Err(AppError::Diarization(
            "segmentation metadata window size is too small for the model sliding-window ratio"
                .to_string(),
        ));
    }
    Ok(window_shift)
}

fn segmentation_progress_units(
    completed_windows: usize,
    total_windows: usize,
) -> Result<(i32, i32)> {
    diarization_progress_units(completed_windows, total_windows, 0)
}

fn embedding_progress_units(completed_windows: usize, total_windows: usize) -> Result<(i32, i32)> {
    diarization_progress_units(completed_windows, total_windows, total_windows)
}

fn diarization_progress_units(
    completed_windows: usize,
    total_windows: usize,
    offset: usize,
) -> Result<(i32, i32)> {
    let total = total_windows.checked_mul(2).ok_or_else(|| {
        AppError::Diarization("diarization progress total overflowed".to_string())
    })?;
    let completed = offset
        .checked_add(completed_windows.min(total_windows))
        .ok_or_else(|| AppError::Diarization("diarization progress overflowed".to_string()))?;
    Ok((
        i32::try_from(completed)
            .map_err(|_| AppError::Diarization("diarization progress exceeds i32".to_string()))?,
        i32::try_from(total)
            .map_err(|_| AppError::Diarization("diarization progress exceeds i32".to_string()))?,
    ))
}

#[derive(Debug, Clone)]
struct SegmentationMetadata {
    sample_rate: u32,
    window_size: usize,
    receptive_field_shift: usize,
    num_speakers: usize,
    powerset_max_classes: usize,
    num_classes: usize,
}

#[derive(Debug)]
struct SegmentationWindow {
    start_sample: usize,
    activity: Vec<Vec<bool>>,
}

struct DirectSegmentationModel {
    session: Session,
    metadata: SegmentationMetadata,
}

impl DirectSegmentationModel {
    fn load(path: &Path) -> Result<Self> {
        let environment = ort_environment()?;
        let session = SessionBuilder::new(&environment)
            .map_err(|error| {
                AppError::Diarization(format!("could not create ONNX session: {error}"))
            })?
            .with_optimization_level(GraphOptimizationLevel::Level1)
            .map_err(|error| {
                AppError::Diarization(format!("could not configure ONNX session: {error}"))
            })?
            .with_intra_threads(1)
            .map_err(|error| {
                AppError::Diarization(format!("could not set ONNX thread count: {error}"))
            })?
            .with_model_from_file(path)
            .map_err(|error| {
                AppError::Diarization(format!("could not load segmentation model: {error}"))
            })?;
        let metadata = segmentation_metadata(&session)?;
        Ok(Self { session, metadata })
    }

    fn infer(
        &self,
        samples: &[f32],
        progress: &mut Option<ProgressCallback>,
    ) -> Result<Vec<SegmentationWindow>> {
        let windows = segmentation_windows(
            samples,
            self.metadata.window_size,
            segmentation_window_shift(self.metadata.window_size)?,
        )?;
        let total = windows.len();
        let mut result = Vec::with_capacity(total);

        for (batch_index, batch) in windows.chunks(SEGMENTATION_BATCH_SIZE).enumerate() {
            let mut values = Vec::with_capacity(batch.len() * self.metadata.window_size);
            for (_, window) in batch {
                values.extend_from_slice(window);
            }
            let array = Array3::from_shape_vec((batch.len(), 1, self.metadata.window_size), values)
                .map_err(|error| {
                    AppError::Diarization(format!("could not shape segmentation input: {error}"))
                })?;
            let input = CowArray::from(array).into_dyn();
            let input = Value::from_array(self.session.allocator(), &input).map_err(|error| {
                AppError::Diarization(format!("could not create segmentation tensor: {error}"))
            })?;
            let outputs = self.session.run(vec![input]).map_err(|error| {
                AppError::Diarization(format!("segmentation inference failed: {error}"))
            })?;
            let output: OrtOwnedTensor<f32, _> = outputs
                .first()
                .ok_or_else(|| {
                    AppError::Diarization("segmentation model returned no outputs".to_string())
                })?
                .try_extract()
                .map_err(|error| {
                    AppError::Diarization(format!("could not read segmentation output: {error}"))
                })?;
            let output = output.view();
            let shape = output.shape();
            if shape.len() != 3 || shape[0] != batch.len() || shape[2] != self.metadata.num_classes
            {
                return Err(AppError::Diarization(format!(
                    "segmentation output shape {:?} does not match batch {} and class count {}",
                    shape,
                    batch.len(),
                    self.metadata.num_classes
                )));
            }
            for (window_index, (start_sample, _)) in batch.iter().enumerate() {
                let mut activity = Vec::with_capacity(shape[1]);
                for frame_index in 0..shape[1] {
                    let logits = output.slice(s![window_index, frame_index, ..]);
                    let class = logits
                        .iter()
                        .enumerate()
                        .max_by(|(_, left), (_, right)| left.total_cmp(right))
                        .map(|(class, _)| class)
                        .ok_or_else(|| {
                            AppError::Diarization("segmentation frame has no classes".to_string())
                        })?;
                    activity.push(powerset_class_to_activity(
                        class,
                        self.metadata.num_speakers,
                        self.metadata.powerset_max_classes,
                    )?);
                }
                result.push(SegmentationWindow {
                    start_sample: *start_sample,
                    activity,
                });
            }
            if let Some(callback) = progress.as_ref() {
                let completed = ((batch_index + 1) * SEGMENTATION_BATCH_SIZE).min(total);
                let (done, progress_total) = segmentation_progress_units(completed, total)?;
                callback(done, progress_total);
            }
        }
        Ok(result)
    }
}

fn segmentation_metadata(session: &Session) -> Result<SegmentationMetadata> {
    let metadata = session.metadata().map_err(|error| {
        AppError::Diarization(format!("could not read segmentation metadata: {error}"))
    })?;
    let value = |key: &str| -> Result<usize> {
        metadata
            .custom(key)
            .map_err(|error| {
                AppError::Diarization(format!(
                    "could not read segmentation metadata {key}: {error}"
                ))
            })?
            .ok_or_else(|| {
                AppError::Diarization(format!("segmentation model lacks metadata key {key}"))
            })?
            .parse::<usize>()
            .map_err(|error| {
                AppError::Diarization(format!("segmentation metadata {key} is invalid: {error}"))
            })
    };
    Ok(SegmentationMetadata {
        sample_rate: value("sample_rate")? as u32,
        window_size: value("window_size")?,
        receptive_field_shift: value("receptive_field_shift")?,
        num_speakers: value("num_speakers")?,
        powerset_max_classes: value("powerset_max_classes")?,
        num_classes: value("num_classes")?,
    })
}

fn configure_ort_dylib() -> Result<()> {
    if ORT_DYLIB_PATH.get().is_some() {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|error| {
        AppError::Diarization(format!(
            "could not locate current executable for ONNX runtime: {error}"
        ))
    })?;
    let exe_dir = exe.parent().ok_or_else(|| {
        AppError::Diarization("current executable has no parent directory".to_string())
    })?;
    let mut candidates = vec![exe_dir.join(ORT_DYLIB_NAME)];
    if let Some(parent) = exe_dir.parent() {
        candidates.push(parent.join(ORT_DYLIB_NAME));
        candidates.push(parent.join("Frameworks").join(ORT_DYLIB_NAME));
    }
    let path = candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            AppError::Diarization(format!("could not locate bundled {ORT_DYLIB_NAME}"))
        })?;
    std::env::set_var("ORT_DYLIB_PATH", &path);
    let _ = ORT_DYLIB_PATH.set(path);
    Ok(())
}

fn initialize_once<'cache, T>(
    cache: &'cache OnceLock<T>,
    init_lock: &Mutex<()>,
    initialize: impl FnOnce() -> Result<T>,
) -> Result<&'cache T> {
    if let Some(value) = cache.get() {
        return Ok(value);
    }

    let _init = init_lock.lock().map_err(|_| {
        AppError::Diarization("ONNX environment initialization lock was poisoned".to_string())
    })?;
    if let Some(value) = cache.get() {
        return Ok(value);
    }

    let _ = cache.set(initialize()?);
    cache.get().ok_or_else(|| {
        AppError::Diarization("ONNX environment was not retained after initialization".to_string())
    })
}

fn ort_environment() -> Result<Arc<Environment>> {
    let environment = initialize_once(&ORT_ENVIRONMENT, &ORT_ENVIRONMENT_INIT, || {
        configure_ort_dylib()?;
        Environment::builder()
            .with_name("whisperpilot-diarization")
            .build()
            .map_err(|error| {
                AppError::Diarization(format!("could not create ONNX environment: {error}"))
            })
            .map(|environment| environment.into_arc())
    })?;
    Ok(Arc::clone(environment))
}

fn diarize_with_rust_clustering(
    models: &DiarizationModelPaths,
    samples: Vec<f32>,
    speaker_count: Option<i32>,
    threshold: f32,
    mut progress: Option<ProgressCallback>,
) -> Result<Vec<SpeakerTurn>> {
    if speaker_count.filter(|count| *count > 0).is_some() {
        return Err(AppError::Diarization(
            "fixed speaker-count diarization is reserved for WP-49".to_string(),
        ));
    }

    let model = DirectSegmentationModel::load(&models.segmentation_model)?;
    if model.metadata.sample_rate != 16_000 {
        return Err(AppError::Diarization(format!(
            "segmentation model expects {} Hz audio, not WhisperPilot's 16000 Hz samples",
            model.metadata.sample_rate
        )));
    }
    let receptive_field_shift = model.metadata.receptive_field_shift;
    let windows = model.infer(&samples, &mut progress)?;
    if windows.is_empty() {
        return Ok(Vec::new());
    }

    let mut extractor = EmbeddingExtractor::new(ExtractorConfig {
        model: models.embedding_model.to_string_lossy().to_string(),
        num_threads: Some(1),
        ..Default::default()
    })
    .map_err(|error| {
        AppError::Diarization(format!("could not initialize embedding extractor: {error}"))
    })?;

    let mut pairs = Vec::new();
    let mut embeddings = Vec::new();
    for (window_index, window) in windows.iter().enumerate() {
        let local_speakers = window.activity.first().map_or(0, Vec::len);
        for local_speaker in 0..local_speakers {
            let active_frames = window
                .activity
                .iter()
                .filter(|activity| activity[local_speaker])
                .count();
            if active_frames < MIN_EMBEDDING_FRAMES {
                continue;
            }
            let audio =
                masked_window_audio(&samples, window, local_speaker, model.metadata.window_size);
            if audio.is_empty() {
                continue;
            }
            let embedding = extractor
                .compute_speaker_embedding(audio, model.metadata.sample_rate)
                .map_err(|error| {
                    AppError::Diarization(format!("could not compute speaker embedding: {error}"))
                })?;
            pairs.push((window_index, local_speaker));
            embeddings.push(embedding);
        }
        if let Some(callback) = progress.as_ref() {
            let (done, total) = embedding_progress_units(window_index + 1, windows.len())?;
            callback(done, total);
        }
    }
    if embeddings.is_empty() {
        return Ok(Vec::new());
    }

    let labels = cluster_embeddings(&embeddings, ClusterStop::Distance(threshold))?;
    let assignments: BTreeMap<(usize, usize), usize> = pairs.into_iter().zip(labels).collect();
    turns_from_assignments(
        &windows,
        &assignments,
        samples.len(),
        receptive_field_shift,
        model.metadata.sample_rate,
    )
}

fn masked_window_audio(
    samples: &[f32],
    window: &SegmentationWindow,
    local_speaker: usize,
    window_size: usize,
) -> Vec<f32> {
    let frame_count = window.activity.len();
    let mut audio = Vec::new();
    for (frame, activity) in window.activity.iter().enumerate() {
        if !activity[local_speaker] {
            continue;
        }
        let start = window.start_sample + frame * window_size / frame_count;
        let end = window.start_sample + (frame + 1) * window_size / frame_count;
        if start < samples.len() {
            audio.extend_from_slice(&samples[start..end.min(samples.len())]);
        }
    }
    audio
}

fn turns_from_assignments(
    windows: &[SegmentationWindow],
    assignments: &BTreeMap<(usize, usize), usize>,
    sample_len: usize,
    receptive_field_shift: usize,
    sample_rate: u32,
) -> Result<Vec<SpeakerTurn>> {
    if receptive_field_shift == 0 || sample_rate == 0 {
        return Err(AppError::Diarization(
            "segmentation metadata has a zero frame shift or sample rate".to_string(),
        ));
    }
    if assignments.is_empty() {
        return Ok(Vec::new());
    }
    let frames = sample_len.div_ceil(receptive_field_shift);
    let cluster_count = assignments.values().copied().max().unwrap_or(0) + 1;
    let mut votes = vec![vec![0u32; cluster_count]; frames];
    let mut local_speaker_counts = vec![0usize; frames];
    let mut window_counts = vec![0usize; frames];
    for (window_index, window) in windows.iter().enumerate() {
        let base = (window.start_sample as f64 / receptive_field_shift as f64).round() as usize;
        for (frame, activity) in window.activity.iter().enumerate() {
            let target = base + frame;
            if target >= votes.len() {
                break;
            }
            local_speaker_counts[target] += activity.iter().filter(|active| **active).count();
            window_counts[target] += 1;
            for (local_speaker, active) in activity.iter().enumerate() {
                if *active {
                    if let Some(cluster) = assignments.get(&(window_index, local_speaker)) {
                        votes[target][*cluster] += 1;
                    }
                }
            }
        }
    }

    let mut activity_by_cluster = vec![vec![false; frames]; cluster_count];
    for frame in 0..frames {
        let window_count = window_counts[frame];
        if window_count == 0 {
            continue;
        }
        let speaker_count = (local_speaker_counts[frame] * 2 + window_count) / (window_count * 2);
        let mut ranked: Vec<(usize, u32)> = votes[frame]
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, count)| *count > 0)
            .collect();
        ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        for (cluster, _) in ranked.into_iter().take(speaker_count) {
            activity_by_cluster[cluster][frame] = true;
        }
    }

    let mut turns = Vec::new();
    for (speaker, activity) in activity_by_cluster.iter_mut().enumerate() {
        smooth_speaker_activity(activity, receptive_field_shift, sample_rate);
        let mut start = 0;
        while start < activity.len() {
            if !activity[start] {
                start += 1;
                continue;
            }
            let mut end = start + 1;
            while end < activity.len() && activity[end] {
                end += 1;
            }
            turns.push(SpeakerTurn {
                start_ms: ((start * receptive_field_shift * 1_000) / sample_rate as usize) as u32,
                end_ms: ((end * receptive_field_shift * 1_000) / sample_rate as usize) as u32,
                speaker: speaker as i32,
            });
            start = end;
        }
    }
    turns.sort_by_key(|turn| turn.start_ms);
    Ok(turns)
}

fn smooth_speaker_activity(activity: &mut [bool], receptive_field_shift: usize, sample_rate: u32) {
    let duration_ms = |frames: usize| -> u32 {
        ((frames * receptive_field_shift * 1_000) / sample_rate as usize) as u32
    };
    let mut index = 0;
    while index < activity.len() {
        if activity[index] {
            index += 1;
            continue;
        }
        let start = index;
        while index < activity.len() && !activity[index] {
            index += 1;
        }
        if start > 0 && index < activity.len() && duration_ms(index - start) <= MAX_SPEAKER_GAP_MS {
            activity[start..index].fill(true);
        }
    }

    let mut index = 0;
    while index < activity.len() {
        if !activity[index] {
            index += 1;
            continue;
        }
        let start = index;
        while index < activity.len() && activity[index] {
            index += 1;
        }
        if duration_ms(index - start) < MIN_TURN_DURATION_MS {
            activity[start..index].fill(false);
        }
    }
}

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

/// One speaker's contiguous span of speech, in milliseconds.
///
/// Serializable because WP-53 runs the engine in a child process and the turns
/// come back as a JSON payload; this is not part of the IPC contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpeakerTurn {
    pub start_ms: u32,
    pub end_ms: u32,
    pub speaker: i32,
}

/// Isolates the real sherpa-onnx engine call so the surrounding logic
/// (config translation, ordering) is unit-testable without real ONNX
/// inference.
#[cfg(test)]
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
#[cfg(test)]
const AUTO_DETECT_THRESHOLD: f32 = 0.9;
#[cfg(test)]
const AUTO_DETECT_MIN_DURATION_ON: f32 = 1.0;
#[cfg(test)]
const AUTO_DETECT_MIN_DURATION_OFF: f32 = 1.0;

/// Translate a caller-provided speaker count into sherpa-onnx's clustering
/// config. `num_clusters < 1` means "auto-detect via the threshold instead"
/// (per sherpa-onnx's `fast-clustering-config.cc`); the crate's own default
/// is a fixed `num_clusters: Some(4)`, so this must be set explicitly.
///
/// `min_duration_on`/`min_duration_off` are gated to the auto-detect branch
/// only — unlike `threshold`, they still apply after clustering regardless
/// of how the count was chosen, so setting them unconditionally would
/// silently affect WP-49's not-yet-built explicit-count path. See
/// docs/architecture.md's Speaker Diarization section for the full rationale.
#[cfg(test)]
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
#[cfg(test)]
fn diarize_with(
    diarizer: &mut impl SpeakerDiarizer,
    samples: Vec<f32>,
) -> Result<Vec<SpeakerTurn>> {
    let mut turns = diarizer.compute(samples)?;
    turns.sort_by_key(|t| t.start_ms);
    Ok(turns)
}

/// Reports direct segmentation and embedding work to the isolated worker's
/// inactivity supervisor. The return value is intentionally advisory: it
/// records liveness but does not cancel native inference.
pub type ProgressCallback = Box<dyn Fn(i32, i32) -> i32 + Send + 'static>;

/// Run speaker diarization over `samples` (16kHz mono f32), using the models
/// WP-5's `resolve_diarization_models` prepares. WP-62 supports automatic
/// threshold clustering only; positive `speaker_count` is rejected until
/// WP-49 owns fixed-count behavior.
///
/// This runs native inference **in this process**; `transcribe_meeting` goes through
/// `diarize_process::diarize_isolated` instead. Retained as the no-progress
/// form used by `tests/diarize_integration.rs` — the isolating child calls
/// [`diarize_samples_with_progress`].
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

/// Overlap duration (ms) between two `[start, end)` spans, or 0 if disjoint.
/// Callers must pass well-formed spans (`start <= end`), matching the
/// invariant `Segment`/`SpeakerTurn` already uphold; a malformed span is a
/// caller bug, not a value this function tries to recover from.
fn overlap_ms(a: (u32, u32), b: (u32, u32)) -> u32 {
    debug_assert!(
        a.0 <= a.1 && b.0 <= b.1,
        "spans must be well-formed (start <= end)"
    );
    let lo = a.0.max(b.0);
    let hi = a.1.min(b.1);
    hi.saturating_sub(lo)
}

/// Temporal gap (ms) between two `[start, end)` spans, or 0 if they touch or
/// overlap. Same well-formed-span precondition as `overlap_ms`.
fn gap_ms(a: (u32, u32), b: (u32, u32)) -> u32 {
    debug_assert!(
        a.0 <= a.1 && b.0 <= b.1,
        "spans must be well-formed (start <= end)"
    );
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
    let (best_speaker, best_overlap) =
        best.expect("turns is non-empty, so overlap_by_speaker is too");

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
///
/// When the outcome carries a fallback warning (the other embedding model
/// was retried after a crash), speakers are still assigned — the warning
/// is informational, not a failure signal.
pub fn apply_diarization_outcome(
    segments: &mut [transcribe::Segment],
    outcome: std::result::Result<
        (Result<Vec<SpeakerTurn>>, Option<String>),
        tokio::task::JoinError,
    >,
) -> Option<String> {
    match outcome {
        Ok((Ok(turns), fallback_warning)) => {
            assign_speaker_ids(segments, &turns);
            fallback_warning
        }
        Ok((Err(e), _fallback_warning)) => {
            log::warn!("diarization unavailable, returning speaker-less segments: {e}");
            Some(format!("Speaker identification is unavailable: {e}"))
        }
        Err(e) => {
            log::warn!("diarization task failed, returning speaker-less segments: {e}");
            Some(format!("Speaker identification failed: {e}"))
        }
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
    fn wp66_uses_a_two_phase_monotonic_progress_total() {
        // Scenario: segmentation and embedding report against one real total.
        assert_eq!(segmentation_progress_units(3, 5).unwrap(), (3, 10));
        assert_eq!(embedding_progress_units(1, 5).unwrap(), (6, 10));
        assert_eq!(embedding_progress_units(5, 5).unwrap(), (10, 10));
    }

    #[test]
    fn wp66_environment_initialization_factory_runs_once() {
        let cache = OnceLock::new();
        let init_lock = Mutex::new(());
        let calls = std::sync::atomic::AtomicUsize::new(0);

        let first = initialize_once(&cache, &init_lock, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(String::from("initialized"))
        })
        .unwrap();
        let second = initialize_once(&cache, &init_lock, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(String::from("must not replace the cached environment"))
        })
        .unwrap();

        assert_eq!(first, "initialized");
        assert_eq!(second, "initialized");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn wp66_derives_the_model_inference_shift_separately_from_frame_timing() {
        // BVA: the model's 160,000 sample window advances by 10%.
        assert_eq!(segmentation_window_shift(160_000).unwrap(), 16_000);
        assert!(matches!(
            segmentation_window_shift(0),
            Err(AppError::Diarization(_))
        ));
    }

    #[test]
    fn wp66_numerical_helpers_reject_invalid_vectors_and_preserve_cosine_bounds() {
        // EP: non-finite and zero vectors are invalid embedding classes.
        assert!(matches!(
            normalize_embedding(&[f32::NAN, 1.0], 0),
            Err(AppError::Diarization(_))
        ));
        assert!(matches!(
            normalize_embedding(&[0.0, 0.0], 0),
            Err(AppError::Diarization(_))
        ));
        assert!(matches!(
            weighted_normalized_centroid(&[1.0, 0.0], 1, &[-1.0, 0.0]),
            Err(AppError::Diarization(_))
        ));

        // BVA: identical, orthogonal, and opposite unit vectors bound cosine distance.
        assert_eq!(cosine_distance(&[1.0, 0.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine_distance(&[1.0, 0.0], &[0.0, 1.0]), 1.0);
        assert_eq!(cosine_distance(&[1.0, 0.0], &[-1.0, 0.0]), 2.0);
    }

    #[test]
    fn wp66_segmentation_and_powerset_helpers_cover_zero_and_empty_boundaries() {
        // EP/BVA: invalid zero metadata and empty audio have distinct outcomes.
        assert!(matches!(
            segmentation_windows(&[1.0], 0, 1),
            Err(AppError::Diarization(_))
        ));
        assert!(matches!(
            segmentation_windows(&[1.0], 1, 0),
            Err(AppError::Diarization(_))
        ));
        assert!(segmentation_windows(&[], 4, 2).unwrap().is_empty());

        // EP: pyannote supports at most two simultaneously-active local speakers.
        assert!(matches!(
            powerset_class_to_activity(0, 0, 2),
            Err(AppError::Diarization(_))
        ));
        assert!(matches!(
            powerset_class_to_activity(0, 3, 0),
            Err(AppError::Diarization(_))
        ));
        assert!(matches!(
            powerset_class_to_activity(0, 3, 3),
            Err(AppError::Diarization(_))
        ));
    }

    #[test]
    fn wp67_turn_reconstruction_merges_short_gaps_and_drops_short_runs() {
        // BVA: a 200ms gap must merge; a 200ms competing run must be dropped.
        let windows = [SegmentationWindow {
            start_sample: 0,
            activity: vec![
                vec![true, false],
                vec![true, false],
                vec![false, true],
                vec![false, true],
                vec![true, false],
                vec![true, false],
                vec![true, false],
                vec![true, false],
                vec![true, false],
                vec![true, false],
            ],
        }];
        let assignments = BTreeMap::from([((0, 0), 0), ((0, 1), 1)]);

        assert_eq!(
            turns_from_assignments(&windows, &assignments, 1_000, 100, 1_000).unwrap(),
            vec![SpeakerTurn {
                start_ms: 0,
                end_ms: 1_000,
                speaker: 0,
            }]
        );
    }

    #[test]
    fn wp67_turn_reconstruction_keeps_overlapping_speakers() {
        // EP: a frame with two local speakers must retain both global labels.
        let windows = [SegmentationWindow {
            start_sample: 0,
            activity: vec![vec![true, true]; 5],
        }];
        let assignments = BTreeMap::from([((0, 0), 0), ((0, 1), 1)]);

        assert_eq!(
            turns_from_assignments(&windows, &assignments, 500, 100, 1_000).unwrap(),
            vec![
                SpeakerTurn {
                    start_ms: 0,
                    end_ms: 500,
                    speaker: 0,
                },
                SpeakerTurn {
                    start_ms: 0,
                    end_ms: 500,
                    speaker: 1,
                },
            ]
        );
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
        assert_eq!(
            merge_segments_with_turns(&after_both, &turns),
            vec![Some(2)]
        );
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

        assert_eq!(merge_segments_with_turns(&segments, &[]), vec![None, None]);
    }

    #[test]
    fn merge_returns_empty_for_empty_segments() {
        let turns = [speaker_turn(0, 1_000, 1)];

        assert_eq!(
            merge_segments_with_turns(&[], &turns),
            Vec::<Option<i32>>::new()
        );
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
        let mut segments = [
            transcript_segment(0, 1_000),
            transcript_segment(2_000, 3_000),
        ];

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
    async fn apply_diarization_outcome_assigns_speakers_and_returns_no_warning_on_success() {
        let mut segments = [transcript_segment(0, 1_000)];
        let turns = vec![speaker_turn(0, 1_000, 3)];

        let warning = apply_diarization_outcome(&mut segments, Ok((Ok(turns), None)));

        assert_eq!(segments[0].speaker_id, Some(3));
        assert_eq!(warning, None);
    }

    #[tokio::test]
    async fn apply_diarization_outcome_assigns_speakers_and_returns_the_fallback_warning() {
        let mut segments = [transcript_segment(0, 1_000)];
        let turns = vec![speaker_turn(0, 1_000, 3)];
        let fallback = "used CAM++ because TitaNet-large failed on this recording".to_string();

        let warning = apply_diarization_outcome(
            &mut segments,
            Ok((Ok(turns.clone()), Some(fallback.clone()))),
        );

        assert_eq!(segments[0].speaker_id, Some(3));
        assert_eq!(warning, Some(fallback));
    }

    #[tokio::test]
    async fn apply_diarization_outcome_leaves_segments_untouched_and_returns_a_warning_on_diarization_asset_error(
    ) {
        let mut segments = [transcript_segment(0, 1_000)];

        let warning = apply_diarization_outcome(
            &mut segments,
            Ok((
                Err(AppError::DiarizationAsset(
                    "models not downloaded".to_string(),
                )),
                None,
            )),
        );

        assert_eq!(segments[0].speaker_id, None);
        assert!(warning.is_some());
    }

    #[tokio::test]
    async fn apply_diarization_outcome_leaves_segments_untouched_and_returns_a_warning_on_engine_error(
    ) {
        let mut segments = [transcript_segment(0, 1_000)];

        let warning = apply_diarization_outcome(
            &mut segments,
            Ok((
                Err(AppError::Diarization("engine exploded".to_string())),
                None,
            )),
        );

        assert_eq!(segments[0].speaker_id, None);
        assert!(warning.is_some());
    }

    #[tokio::test]
    async fn apply_diarization_outcome_leaves_segments_untouched_and_returns_a_warning_when_the_task_panics(
    ) {
        let mut segments = [transcript_segment(0, 1_000)];

        let join_result = tokio::task::spawn_blocking(|| {
            panic!("simulated diarization task panic");
        })
        .await;
        let err = join_result.expect_err("spawned task was made to panic");

        let warning = apply_diarization_outcome(&mut segments, Err(err));

        assert_eq!(segments[0].speaker_id, None);
        assert!(warning.is_some());
    }
}
