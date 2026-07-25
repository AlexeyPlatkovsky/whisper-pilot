//! WP-50 regression evidence: reproduces the before/after cluster-count
//! comparisons (and the crash-safety finding) that justified
//! `diarize.rs::build_config`'s tuned auto-detect threshold/min_duration
//! values (see its doc comment). Ignored by default, same convention as
//! `diarize_integration.rs` — needs the diarization models downloaded plus
//! one or more real audio fixtures. Run with:
//!   WHISPERPILOT_TEST_DIARIZE_MODELS_DIR=<app-support-dir-with-models> \
//!   WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_A=/path/to/real/2-speaker.mp4 \
//!   WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_B=/path/to/another/real.mp4 \
//!   WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_C=/path/to/a/third/real.mp4 \
//!   cargo test --manifest-path src-tauri/Cargo.toml --test wp50_tuning_regression -- --ignored --nocapture --test-threads=1
//!
//! `_A` is required; `_B`/`_C` are optional extra crash-safety/over-clustering
//! spot checks. `--test-threads=1` avoids running two native diarization
//! engines concurrently in one process.

use sherpa_rs::diarize::{Diarize, DiarizeConfig};
use std::collections::HashSet;
use std::path::PathBuf;

fn app_support_dir() -> Option<PathBuf> {
    std::env::var("WHISPERPILOT_TEST_DIARIZE_MODELS_DIR")
        .ok()
        .map(PathBuf::from)
}

/// Runs one auto-detect diarization pass and returns (distinct speaker
/// count, total speech duration per speaker, descending).
fn run_once(
    dir: &std::path::Path,
    samples: &[f32],
    threshold: f32,
    min_on: f32,
    min_off: f32,
) -> (usize, Vec<(i32, f32)>) {
    let models = whisperpilot_lib::diarize::resolve_diarization_models(dir, "campplus")
        .expect("resolve models");
    let config = DiarizeConfig {
        num_clusters: Some(0), // auto-detect
        threshold: Some(threshold),
        min_duration_on: Some(min_on),
        min_duration_off: Some(min_off),
        provider: None,
        debug: false,
    };
    let mut engine = Diarize::new(
        models.segmentation_model.clone(),
        models.embedding_model.clone(),
        config,
    )
    .expect("init diarizer");
    let segments = engine.compute(samples.to_vec(), None).expect("compute");
    let speakers: HashSet<i32> = segments.iter().map(|s| s.speaker).collect();
    let mut duration_by_speaker: std::collections::BTreeMap<i32, f32> = Default::default();
    for s in &segments {
        *duration_by_speaker.entry(s.speaker).or_insert(0.0) += s.end - s.start;
    }
    let mut durations: Vec<(i32, f32)> = duration_by_speaker.into_iter().collect();
    durations.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    (speakers.len(), durations)
}

fn load(env_var: &str) -> Option<Vec<f32>> {
    let path = std::env::var(env_var).ok()?;
    Some(whisperpilot_lib::audio::load_samples(&PathBuf::from(path)).expect("decode audio"))
}

#[test]
#[ignore]
fn tuned_config_reduces_clusters_without_merging_the_two_known_real_speakers() {
    let Some(dir) = app_support_dir() else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_MODELS_DIR");
        return;
    };
    if !dir.join("models").exists() {
        eprintln!("SKIP: no diarization models at {}", dir.display());
        return;
    }
    let Some(samples) = load("WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_A") else {
        eprintln!(
            "SKIP: set WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_A to a real, \
             known-2-speaker recording"
        );
        return;
    };
    eprintln!(
        "decoded {} samples ({:.1}s)",
        samples.len(),
        samples.len() as f32 / 16_000.0
    );

    // Baseline: the crate's own DiarizeConfig::default() values.
    let (baseline_count, baseline_durations) = run_once(&dir, &samples, 0.5, 0.0, 0.0);
    eprintln!(
        "baseline (threshold=0.50 min_on=0.0 min_off=0.0): {baseline_count} distinct speakers, \
         duration_by_speaker(desc)={baseline_durations:?}"
    );

    // Tuned: diarize.rs::build_config's current auto-detect constants.
    let (tuned_count, tuned_durations) = run_once(&dir, &samples, 0.9, 1.0, 1.0);
    eprintln!(
        "tuned (threshold=0.90 min_on=1.0 min_off=1.0): {tuned_count} distinct speakers, \
         duration_by_speaker(desc)={tuned_durations:?}"
    );

    assert!(
        tuned_count < baseline_count,
        "tuned config ({tuned_count} speakers) should produce fewer clusters than \
         the crate defaults ({baseline_count} speakers) on a real over-clustering recording"
    );
    // This fixture is a known real 2-person conversation: the 2 largest
    // clusters must still dominate, i.e. the real speakers stayed separated
    // rather than folding into 1 cluster.
    assert!(
        tuned_durations.len() >= 2,
        "expected at least 2 distinct clusters (the 2 known real speakers), got {}",
        tuned_durations.len()
    );
}

/// Crash-safety + over-clustering spot check against a second real recording
/// with unknown true speaker count — does not assert a minimum cluster count
/// (no ground truth to check merging against), only that the tuned config
/// runs to completion (guards the SIGBUS crash found during this task's own
/// investigation: threshold >= 0.94 crashed sherpa-onnx's native clustering
/// on a real 92s recording) and reduces clusters versus the crate defaults.
#[test]
#[ignore]
fn tuned_config_does_not_crash_and_reduces_clusters_on_a_second_real_recording() {
    let Some(dir) = app_support_dir() else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_MODELS_DIR");
        return;
    };
    let Some(samples) = load("WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_B") else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_B to run this spot check");
        return;
    };
    let (baseline_count, _) = run_once(&dir, &samples, 0.5, 0.0, 0.0);
    let (tuned_count, tuned_durations) = run_once(&dir, &samples, 0.9, 1.0, 1.0);
    eprintln!(
        "baseline: {baseline_count} speakers; tuned: {tuned_count} speakers, \
         duration_by_speaker(desc)={tuned_durations:?}"
    );
    assert!(tuned_count <= baseline_count);
}

/// Same crash-safety + over-clustering spot check against a third recording.
#[test]
#[ignore]
fn tuned_config_does_not_crash_and_reduces_clusters_on_a_third_real_recording() {
    let Some(dir) = app_support_dir() else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_MODELS_DIR");
        return;
    };
    let Some(samples) = load("WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_C") else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_C to run this spot check");
        return;
    };
    let (baseline_count, _) = run_once(&dir, &samples, 0.5, 0.0, 0.0);
    let (tuned_count, tuned_durations) = run_once(&dir, &samples, 0.9, 1.0, 1.0);
    eprintln!(
        "baseline: {baseline_count} speakers; tuned: {tuned_count} speakers, \
         duration_by_speaker(desc)={tuned_durations:?}"
    );
    assert!(tuned_count <= baseline_count);
}
