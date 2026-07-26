//! WP-53 regression: the recording that reliably kills the process inside
//! sherpa-onnx's native clustering must now come back as a contained failure.
//!
//! Before this work, `2026-07-21 12-07-19.mp4` + `titanet-large` + the shipped
//! threshold of 0.9 raised SIGBUS on a tokio worker thread and took the whole
//! app down with it — no Rust error path could observe it, so the fail-open
//! contract in `apply_diarization_outcome` never got the chance to run. With
//! the engine call isolated in a child process, the same input must return a
//! `ChildOutcome::Crashed` (and, through `into_result`, an ordinary
//! `AppError::Diarization`) while this test process survives to assert it.
//!
//! Ignored by default, following `diarize_integration.rs` and
//! `wp50_tuning_regression.rs`: it needs the real diarization models
//! downloaded and a real recording. Run with:
//!   cargo test --manifest-path src-tauri/Cargo.toml \
//!     --test diarize_isolation_regression -- --ignored --nocapture

use std::path::PathBuf;
use std::time::Duration;

use whisperpilot_lib::diarize_process::{self, ChildOutcome};

/// The app-support-shaped directory holding `models/<catalog file names>`.
/// Override with WHISPERPILOT_TEST_DIARIZE_MODELS_DIR; falls back to the real
/// macOS app-support dir Tauri resolves at runtime, matching where WP-39
/// downloads land and mirroring `diarize_integration.rs`.
fn app_support_dir() -> PathBuf {
    if let Ok(p) = std::env::var("WHISPERPILOT_TEST_DIARIZE_MODELS_DIR") {
        return PathBuf::from(p);
    }
    dirs_next::data_local_dir()
        .or_else(|| dirs_next::home_dir().map(|h| h.join("Library/Application Support")))
        .unwrap_or_default()
        .join("com.whisperpilot.dev")
}

/// The known crash-triggering recording, already decoded to raw 16kHz mono
/// f32 PCM. Override with WHISPERPILOT_TEST_CRASH_SAMPLES; produce it with:
///   ffmpeg -i "2026-07-21 12-07-19.mp4" -f f32le -ac 1 -ar 16000 crash.f32
fn crash_samples_path() -> Option<PathBuf> {
    std::env::var("WHISPERPILOT_TEST_CRASH_SAMPLES")
        .ok()
        .map(PathBuf::from)
}

fn read_raw_f32(path: &PathBuf) -> Vec<f32> {
    std::fs::read(path)
        .unwrap_or_else(|e| panic!("reading raw pcm fixture {}: {e}", path.display()))
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// The worker is the app binary itself re-executed in its hidden argv mode
/// (one binary, one code signature). Cargo builds it for this integration
/// test and hands us its path, standing in for `current_exe()` — which under
/// `cargo test` would be the test binary rather than the app.
fn worker_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_whisper-pilot"))
}

#[test]
#[ignore]
fn the_known_crashing_recording_no_longer_takes_the_process_down() {
    let Some(samples_path) = crash_samples_path() else {
        eprintln!("set WHISPERPILOT_TEST_CRASH_SAMPLES to the decoded reproducer; skipping");
        return;
    };
    let dir = app_support_dir();
    assert!(
        dir.join("models").is_dir(),
        "expected downloaded models under {}",
        dir.display()
    );

    let samples = read_raw_f32(&samples_path);
    assert!(!samples.is_empty(), "reproducer decoded to zero samples");

    let outcome = diarize_process::diarize_isolated_with(
        &worker_exe(),
        &dir,
        samples,
        None,
        "titanet-large",
        Duration::from_secs(120),
    );

    // Reaching this line at all is the regression assertion: before WP-53 the
    // process was gone by now.
    match outcome {
        ChildOutcome::Crashed { signal } => {
            eprintln!("contained the native crash (signal {signal}) — app survived");
            let error = ChildOutcome::Crashed { signal }
                .into_result()
                .expect_err("a crash must degrade to a diarization error");
            assert!(matches!(
                error,
                whisperpilot_lib::error::AppError::Diarization(_)
            ));
        }
        ChildOutcome::Completed(turns) => {
            // Acceptable: the native fault is input- and build-dependent, and
            // a run that completes still proves isolation did not break the
            // engine. It does not prove containment, so say so loudly.
            eprintln!(
                "WARNING: this input did not crash on this build ({} turns); \
                 containment was not exercised",
                turns.len()
            );
        }
        other => panic!("expected a contained crash or a clean run, got {other:?}"),
    }
}

/// Containment is only half the contract: a run that does *not* crash must
/// still come back with real speaker turns through the child's transport.
/// campplus completes on this same recording at threshold 0.9 (WP-53 triage),
/// so it exercises the success path end-to-end with the real engine.
#[test]
#[ignore]
fn a_surviving_model_still_returns_real_turns_through_the_child() {
    let Some(samples_path) = crash_samples_path() else {
        eprintln!("set WHISPERPILOT_TEST_CRASH_SAMPLES to the decoded reproducer; skipping");
        return;
    };
    let dir = app_support_dir();
    let samples = read_raw_f32(&samples_path);

    let outcome = diarize_process::diarize_isolated_with(
        &worker_exe(),
        &dir,
        samples,
        None,
        "campplus",
        Duration::from_secs(120),
    );

    match outcome {
        ChildOutcome::Completed(turns) => {
            assert!(!turns.is_empty(), "an isolated success must carry its turns");
            assert!(
                turns.windows(2).all(|w| w[0].start_ms <= w[1].start_ms),
                "turns must survive the process boundary still ordered"
            );
            eprintln!("isolated run produced {} turns", turns.len());
        }
        other => panic!("campplus is expected to complete on this input, got {other:?}"),
    }
}
