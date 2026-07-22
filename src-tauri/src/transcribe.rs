//! Offline, full-file transcription with whisper-rs on Metal.
//!
//! Unlike a live pipeline, the whole file is decoded in one pass with beam
//! search and no real-time constraint, so accuracy is the only priority.
//! Language defaults to Russian.

use crate::error::{AppError, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// One transcript segment with its time span, in milliseconds from file start.
#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Load the whisper model from the WP-39 download location under
/// `app_support_dir`; override with `WHISPERPILOT_MODEL_PATH`.
pub fn load_model(app_support_dir: &Path) -> Result<WhisperContext> {
    let path = model_path(app_support_dir);
    if !path.exists() {
        return Err(AppError::ModelNotFound(path.display().to_string()));
    }
    let mut params = WhisperContextParameters::default();
    params.flash_attn(true);
    let path_str = path
        .to_str()
        .ok_or_else(|| AppError::ModelLoad("model path is not valid UTF-8".into()))?;
    WhisperContext::new_with_params(path_str, params)
        .map_err(|e| AppError::ModelLoad(e.to_string()))
}

fn model_path(app_support_dir: &Path) -> PathBuf {
    resolve_model_path(app_support_dir, std::env::var("WHISPERPILOT_MODEL_PATH").ok())
}

/// Pure decision: an explicit override always wins; otherwise the model
/// lives where WP-39's downloader put it. No env access here, so this is
/// unit-testable without mutating global state.
fn resolve_model_path(app_support_dir: &Path, override_path: Option<String>) -> PathBuf {
    if let Some(p) = override_path {
        return PathBuf::from(p);
    }
    crate::models::primary_asset_path(app_support_dir, "transcription")
        .expect("\"transcription\" is a static CATALOG entry with at least one asset")
}

/// Transcribe an entire file of 16 kHz mono samples into timestamped segments.
pub fn transcribe(ctx: &WhisperContext, samples: &[f32], language: &str) -> Result<Vec<Segment>> {
    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: -1.0,
    });
    params.set_n_threads(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4) as i32,
    );
    params.set_language(Some(language));
    params.set_translate(false);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    // Offline: let whisper segment naturally, and lean on its fallbacks.
    params.set_temperature_inc(0.2);
    params.set_suppress_blank(true);

    let mut state = ctx
        .create_state()
        .map_err(|e| AppError::Transcribe(e.to_string()))?;
    state
        .full(params, samples)
        .map_err(|e| AppError::Transcribe(e.to_string()))?;

    let n = state.full_n_segments();
    let mut segments = Vec::with_capacity(n as usize);
    for i in 0..n {
        let Some(seg) = state.get_segment(i) else {
            continue;
        };
        let text = seg
            .to_str_lossy()
            .map_err(|e| AppError::Transcribe(e.to_string()))?
            .trim()
            .to_owned();
        if text.is_empty() {
            continue;
        }
        // whisper timestamps are centiseconds → milliseconds.
        segments.push(Segment {
            start_ms: seg.start_timestamp().max(0) as u64 * 10,
            end_ms: seg.end_timestamp().max(0) as u64 * 10,
            text,
        });
    }

    Ok(segments)
}

/// Full path from a picked file to timestamped segments.
pub fn transcribe_file(ctx: &WhisperContext, input: &Path, language: &str) -> Result<Vec<Segment>> {
    let samples = crate::audio::load_samples(input)?;
    transcribe(ctx, &samples, language)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_model_path_uses_the_explicit_override_when_set() {
        let dir = tempfile::tempdir().unwrap();

        let path = resolve_model_path(dir.path(), Some("/custom/path/model.bin".to_string()));

        assert_eq!(path, PathBuf::from("/custom/path/model.bin"));
    }

    #[test]
    fn resolve_model_path_defaults_to_the_wp39_download_location() {
        let dir = tempfile::tempdir().unwrap();

        let path = resolve_model_path(dir.path(), None);

        assert_eq!(
            path,
            dir.path().join("models").join("ggml-large-v3-turbo-q8_0.bin")
        );
    }
}
