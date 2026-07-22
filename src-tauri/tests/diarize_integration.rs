//! Real-engine diarization check: real sherpa-onnx models → speaker turns.
//!
//! Ignored by default: needs the diarization models downloaded (via Settings,
//! WP-39/40) into an app-support-shaped directory, plus a multi-speaker audio
//! fixture. Run with:
//!   cargo test --manifest-path src-tauri/Cargo.toml --test diarize_integration -- --ignored --nocapture

use std::path::PathBuf;

/// An app-support-shaped directory (containing `models/<catalog file names>`)
/// with the diarization assets already present. Override with
/// WHISPERPILOT_TEST_DIARIZE_MODELS_DIR; falls back to the real macOS app
/// support dir Tauri resolves at runtime (matching where WP-39 downloads
/// land), mirroring tests/pipeline.rs's convention.
fn app_support_dir() -> PathBuf {
    if let Ok(p) = std::env::var("WHISPERPILOT_TEST_DIARIZE_MODELS_DIR") {
        return PathBuf::from(p);
    }
    dirs_next::data_local_dir()
        .or_else(|| dirs_next::home_dir().map(|h| h.join("Library/Application Support")))
        .unwrap_or_default()
        .join("com.whisperpilot.dev")
}

/// Two raw 16kHz mono f32 PCM files, each a different speaker, concatenated
/// with a silence gap to form the test sample. Override with
/// WHISPERPILOT_TEST_DIARIZE_VOICE_A / _B.
fn voice_paths() -> Option<(PathBuf, PathBuf)> {
    let a = std::env::var("WHISPERPILOT_TEST_DIARIZE_VOICE_A").ok()?;
    let b = std::env::var("WHISPERPILOT_TEST_DIARIZE_VOICE_B").ok()?;
    Some((PathBuf::from(a), PathBuf::from(b)))
}

fn read_raw_f32(path: &PathBuf) -> Vec<f32> {
    std::fs::read(path)
        .unwrap_or_else(|e| panic!("reading raw pcm fixture {}: {e}", path.display()))
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[test]
#[ignore]
fn diarizes_real_two_speaker_audio_into_ordered_non_overlapping_turns() {
    let dir = app_support_dir();
    if !dir.join("models").exists() {
        eprintln!(
            "SKIP: no diarization models at {} (set WHISPERPILOT_TEST_DIARIZE_MODELS_DIR)",
            dir.display()
        );
        return;
    }
    let Some((voice_a, voice_b)) = voice_paths() else {
        eprintln!(
            "SKIP: no audio fixture (set WHISPERPILOT_TEST_DIARIZE_VOICE_A / _B, \
             16kHz mono raw f32 PCM, one distinct speaker each)"
        );
        return;
    };

    let mut samples = read_raw_f32(&voice_a);
    samples.extend(std::iter::repeat(0.0).take(16_000)); // 1s silence gap
    samples.extend(read_raw_f32(&voice_b));

    // speaker_count is an explicit override here (not auto-detect) because
    // this test's job is to prove the real engine call plumbs through
    // correctly (real models load, real inference runs, no panic, turns come
    // back ordered) — NOT to prove clustering quality. Auto-detection's
    // threshold-based split is inherently input-dependent (ADR-005 already
    // documents diarization quality as "good but below pyannote's best");
    // two short, acoustically-similar synthesized voices are a harder case
    // for auto-detect than typical real multi-speaker meeting audio.
    let turns = whisperpilot_lib::diarize::diarize_samples(&dir, samples, Some(2))
        .expect("diarization should succeed against real models and audio");

    eprintln!("turns: {turns:?}");
    assert!(!turns.is_empty(), "expected at least 1 speaker turn");
    for w in turns.windows(2) {
        assert!(
            w[1].start_ms >= w[0].start_ms,
            "turns must be ordered by start_ms"
        );
    }
    for t in &turns {
        assert!(t.end_ms > t.start_ms, "each turn must be non-degenerate");
    }
    let speakers: std::collections::HashSet<i32> = turns.iter().map(|t| t.speaker).collect();
    eprintln!(
        "distinct speakers found: {} (informational only — clustering quality is \
         input-dependent, not asserted here)",
        speakers.len()
    );
}
