//! WP-84 regression: the decode callbacks must not abort runs at random.
//!
//! whisper-rs's `set_abort_callback_safe` / `set_progress_callback_safe`
//! reinterpret the boxed fat pointer as the concrete closure type (UB), so a
//! cancel flag that stays `false` could still abort the encode loop with
//! whisper error -6 — do not use them; see WP-84.
//!
//! Ignored by default: needs the whisper model on disk and an audio fixture
//! via WHISPERPILOT_TEST_AUDIO. Runs on the CPU backend deliberately — an
//! unrelated intermittent ggml Metal fault would otherwise make the outcome
//! indistinguishable. Run with:
//!   WHISPERPILOT_TEST_AUDIO=<sample.wav> cargo test --manifest-path src-tauri/Cargo.toml --test wp84_callback_regression -- --ignored --nocapture

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use whisper_rs::{WhisperContext, WhisperContextParameters};
use whisperpilot_lib::error::AppError;

/// A real audio file to exercise the decode, in any language.
fn sample_path() -> Option<PathBuf> {
    std::env::var("WHISPERPILOT_TEST_AUDIO")
        .ok()
        .map(PathBuf::from)
}

/// The real macOS app support dir Tauri resolves at runtime for this app's
/// bundle identifier (com.whisperpilot.dev); matches where WP-39 downloads land.
fn app_support_dir() -> PathBuf {
    dirs_next::data_local_dir()
        .or_else(|| dirs_next::home_dir().map(|h| h.join("Library/Application Support")))
        .unwrap_or_default()
        .join("com.whisperpilot.dev")
}

/// A CPU-decode context over the production model, so the stress runs measure
/// the callback wiring and not the unrelated intermittent Metal fault.
fn cpu_context() -> Option<WhisperContext> {
    let model = app_support_dir()
        .join("models")
        .join("ggml-large-v3-turbo-q8_0.bin");
    let mut params = WhisperContextParameters::default();
    params.use_gpu(false);
    params.flash_attn(true);
    WhisperContext::new_with_params(model.to_str()?, params).ok()
}

/// The WP-84 stress regression: five consecutive decodes with a cancel flag
/// that stays `false` must all complete. Before the fix, the UB trampoline
/// aborted the encode loop at random (`whisper_full_with_state: failed to
/// encode`, error -6).
#[test]
#[ignore]
fn false_cancel_flag_never_aborts_the_decode() {
    let (Some(audio), Some(ctx)) = (sample_path(), cpu_context()) else {
        eprintln!("SKIP: no sample audio or model (set WHISPERPILOT_TEST_AUDIO)");
        return;
    };
    let samples = whisperpilot_lib::audio::load_samples(&audio).expect("samples");

    for run in 1..=5 {
        let cancel = Arc::new(AtomicBool::new(false));
        let result = whisperpilot_lib::transcribe::transcribe(&ctx, &samples, &cancel, |_| {});
        assert!(
            result.is_ok(),
            "run {run}/5 aborted with a false cancel flag: {result:?}"
        );
    }
}

/// The other half of the contract: a flag that is already `true` must still
/// cancel the run, surfacing `AppError::Cancelled` rather than a transcript.
#[test]
#[ignore]
fn set_cancel_flag_cancels_the_run() {
    let (Some(audio), Some(ctx)) = (sample_path(), cpu_context()) else {
        eprintln!("SKIP: no sample audio or model (set WHISPERPILOT_TEST_AUDIO)");
        return;
    };
    let samples = whisperpilot_lib::audio::load_samples(&audio).expect("samples");

    let cancel = Arc::new(AtomicBool::new(true));
    let result = whisperpilot_lib::transcribe::transcribe(&ctx, &samples, &cancel, |_| {});

    assert!(
        matches!(result, Err(AppError::Cancelled)),
        "a set cancel flag must yield AppError::Cancelled, got: {result:?}"
    );
}
