//! WP-62 Route A real-audio measurement sweep.
//!
//! Ignored by default because it needs the user's downloaded diarization
//! models and known-two-speaker recordings. Run with:
//!   WHISPERPILOT_TEST_DIARIZE_MODELS_DIR=<app support dir> \
//!   WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_A=<92s recording> \
//!   WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_B=<861s recording> \
//!   cargo test --manifest-path src-tauri/Cargo.toml --test wp62_route_a_regression -- --ignored --nocapture --test-threads=1

use std::collections::BTreeMap;
use std::path::PathBuf;

use sherpa_rs::diarize::{Diarize, DiarizeConfig};

const SWEEP: [f32; 5] = [0.95, 0.90, 0.85, 0.80, 0.75];
const AUTOMATIC_THRESHOLD: f32 = 0.80;
const EMBEDDING_VARIANTS: [&str; 2] = ["campplus", "titanet-large"];

fn samples(name: &str) -> Option<Vec<f32>> {
    let path = std::env::var(name).ok()?;
    Some(
        whisperpilot_lib::audio::load_samples(&PathBuf::from(path))
            .expect("decode reference recording"),
    )
}

fn metrics(turns: &[whisperpilot_lib::diarize::SpeakerTurn]) -> (usize, f32) {
    let mut by_speaker: BTreeMap<i32, f32> = BTreeMap::new();
    for turn in turns {
        *by_speaker.entry(turn.speaker).or_default() += (turn.end_ms - turn.start_ms) as f32;
    }
    let total: f32 = by_speaker.values().sum();
    let mut durations: Vec<f32> = by_speaker.values().copied().collect();
    durations.sort_by(|left, right| right.total_cmp(left));
    let dominant_two_share = if total > 0.0 {
        durations.iter().take(2).sum::<f32>() / total
    } else {
        0.0
    };
    (by_speaker.len(), dominant_two_share)
}

fn vendored_baseline_turns(
    models_dir: &std::path::Path,
    samples: &[f32],
) -> Vec<whisperpilot_lib::diarize::SpeakerTurn> {
    let models = whisperpilot_lib::diarize::resolve_diarization_models(models_dir, "campplus")
        .expect("resolve baseline models");
    let config = DiarizeConfig {
        num_clusters: Some(0),
        threshold: Some(0.5),
        min_duration_on: Some(0.0),
        min_duration_off: Some(0.0),
        provider: None,
        debug: false,
    };
    let mut engine = Diarize::new(
        models.segmentation_model.clone(),
        models.embedding_model.clone(),
        config,
    )
    .expect("initialize vendored baseline");
    engine
        .compute(samples.to_vec(), None)
        .expect("run vendored baseline")
        .into_iter()
        .map(|turn| whisperpilot_lib::diarize::SpeakerTurn {
            start_ms: (turn.start * 1_000.0).round() as u32,
            end_ms: (turn.end * 1_000.0).round() as u32,
            speaker: turn.speaker,
        })
        .collect()
}

#[test]
#[ignore]
fn s4_route_a_records_the_ordered_two_speaker_threshold_sweep() {
    let Some(models_dir) = std::env::var("WHISPERPILOT_TEST_DIARIZE_MODELS_DIR")
        .ok()
        .map(PathBuf::from)
    else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_MODELS_DIR");
        return;
    };
    let Some(short) = samples("WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_A") else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_A");
        return;
    };
    let Some(long) = samples("WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_B") else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_B");
        return;
    };

    for (name, reference) in [("92.47s", short), ("861.57s", long)] {
        // S-4 decision-table baseline: use the prior production algorithm
        // only in this ignored regression so both requested metrics are
        // captured before Route A replaces it.
        let baseline_turns = vendored_baseline_turns(&models_dir, &reference);
        let (baseline_clusters, baseline_dominant_two_share) = metrics(&baseline_turns);
        eprintln!(
            "{name} vendored baseline: clusters={baseline_clusters}; dominant_two_share={baseline_dominant_two_share:.4}"
        );
        for threshold in SWEEP {
            let turns = whisperpilot_lib::diarize::diarize_samples_with_cluster_threshold(
                &models_dir,
                reference.clone(),
                "campplus",
                threshold,
            )
            .unwrap_or_else(|error| {
                panic!("Route A {name} threshold {threshold:.2} failed: {error}")
            });
            let (clusters, dominant_two_share) = metrics(&turns);
            eprintln!(
                "{name} Route A threshold={threshold:.2}: clusters={clusters}; dominant_two_share={dominant_two_share:.4}; baseline_clusters={baseline_clusters}; baseline_dominant_two_share={baseline_dominant_two_share:.4}",
            );
            assert!(!turns.is_empty(), "Route A must return turns for {name}");
        }
    }
}

#[test]
#[ignore]
fn wp67_automatic_diarization_keeps_two_speakers_and_smooth_turns_for_every_variant() {
    let Some(models_dir) = std::env::var("WHISPERPILOT_TEST_DIARIZE_MODELS_DIR")
        .ok()
        .map(PathBuf::from)
    else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_MODELS_DIR");
        return;
    };
    let Some(short) = samples("WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_A") else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_A");
        return;
    };
    let Some(long) = samples("WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_B") else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_B");
        return;
    };

    for (name, reference) in [("92.47s", short), ("861.57s", long)] {
        for variant in EMBEDDING_VARIANTS {
            let turns = whisperpilot_lib::diarize::diarize_samples_with_cluster_threshold(
                &models_dir,
                reference.clone(),
                variant,
                AUTOMATIC_THRESHOLD,
            )
            .unwrap_or_else(|error| {
                panic!("{name} {variant} automatic diarization failed: {error}")
            });
            let (clusters, dominant_two_share) = metrics(&turns);
            eprintln!(
                "{name} {variant} automatic threshold={AUTOMATIC_THRESHOLD:.2}: clusters={clusters}; dominant_two_share={dominant_two_share:.4}; turns={}",
                turns.len()
            );
            assert_eq!(clusters, 2, "{name} {variant} must retain two speakers");
            assert!(
                turns.iter().all(|turn| turn.end_ms - turn.start_ms >= 300),
                "{name} {variant} must not retain a sub-300ms turn"
            );
        }
    }
}

#[test]
#[ignore]
fn wp67_measures_short_recording_thresholds_for_every_variant() {
    let Some(models_dir) = std::env::var("WHISPERPILOT_TEST_DIARIZE_MODELS_DIR")
        .ok()
        .map(PathBuf::from)
    else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_MODELS_DIR");
        return;
    };
    let Some(short) = samples("WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_A") else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_A");
        return;
    };

    for variant in EMBEDDING_VARIANTS {
        for threshold in [0.85, 0.75, 0.65, 0.55, 0.45] {
            let turns = whisperpilot_lib::diarize::diarize_samples_with_cluster_threshold(
                &models_dir,
                short.clone(),
                variant,
                threshold,
            )
            .unwrap_or_else(|error| panic!("{variant} threshold {threshold:.2} failed: {error}"));
            let (clusters, dominant_two_share) = metrics(&turns);
            eprintln!(
                "92.47s {variant} threshold={threshold:.2}: clusters={clusters}; dominant_two_share={dominant_two_share:.4}; turns={}",
                turns.len()
            );
        }
    }
}

#[test]
#[ignore]
fn wp67_measures_long_recording_at_080_for_every_variant() {
    let Some(models_dir) = std::env::var("WHISPERPILOT_TEST_DIARIZE_MODELS_DIR")
        .ok()
        .map(PathBuf::from)
    else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_MODELS_DIR");
        return;
    };
    let Some(long) = samples("WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_B") else {
        eprintln!("SKIP: set WHISPERPILOT_TEST_DIARIZE_REAL_AUDIO_B");
        return;
    };

    for variant in EMBEDDING_VARIANTS {
        let turns = whisperpilot_lib::diarize::diarize_samples_with_cluster_threshold(
            &models_dir,
            long.clone(),
            variant,
            AUTOMATIC_THRESHOLD,
        )
        .unwrap_or_else(|error| {
            panic!("{variant} threshold {AUTOMATIC_THRESHOLD:.2} failed: {error}")
        });
        let (clusters, dominant_two_share) = metrics(&turns);
        eprintln!(
            "861.57s {variant} threshold={AUTOMATIC_THRESHOLD:.2}: clusters={clusters}; dominant_two_share={dominant_two_share:.4}; turns={}",
            turns.len()
        );
    }
}
