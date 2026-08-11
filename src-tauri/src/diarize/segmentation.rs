//! Direct pyannote-style segmentation model inference over mono samples:
//! model loading, ORT environment setup, sliding-window inference, and
//! powerset-class activity decoding.

use crate::error::{AppError, Result};
use ndarray::{s, Array3, CowArray};
use ort::{
    tensor::OrtOwnedTensor, Environment, GraphOptimizationLevel, Session, SessionBuilder, Value,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const SEGMENTATION_BATCH_SIZE: usize = 32;
// pyannote segmentation 3.0 advances every inference window by ten percent.
// This is distinct from `receptive_field_shift`, which timestamps output frames.
const SEGMENTATION_WINDOW_SHIFT_DIVISOR: usize = 10;
const ORT_DYLIB_NAME: &str = "libonnxruntime.1.17.1.dylib";
static ORT_DYLIB_PATH: OnceLock<PathBuf> = OnceLock::new();
static ORT_ENVIRONMENT: OnceLock<Arc<Environment>> = OnceLock::new();
static ORT_ENVIRONMENT_INIT: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone)]
pub(crate) struct SegmentationMetadata {
    pub(crate) sample_rate: u32,
    pub(crate) window_size: usize,
    pub(crate) receptive_field_shift: usize,
    num_speakers: usize,
    powerset_max_classes: usize,
    num_classes: usize,
}

#[derive(Debug)]
pub(crate) struct SegmentationWindow {
    pub(crate) start_sample: usize,
    pub(crate) activity: Vec<Vec<bool>>,
}

pub(crate) struct DirectSegmentationModel {
    session: Session,
    pub(crate) metadata: SegmentationMetadata,
}

impl DirectSegmentationModel {
    pub(crate) fn load(path: &Path) -> Result<Self> {
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

    pub(crate) fn infer(
        &self,
        samples: &[f32],
        progress: &mut Option<crate::diarize::ProgressCallback>,
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

pub(crate) fn initialize_once<'cache, T>(
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

pub(crate) fn embedding_progress_units(
    completed_windows: usize,
    total_windows: usize,
) -> Result<(i32, i32)> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
