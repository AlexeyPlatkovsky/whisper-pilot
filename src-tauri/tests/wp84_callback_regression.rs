//! Regression coverage for a normal decode with no abort callback.
//!
//! Ignored by default: needs the whisper model on disk and an audio fixture
//! via WHISPERPILOT_TEST_AUDIO. Runs on the CPU backend deliberately — an
//! unrelated intermittent ggml Metal fault would otherwise make the outcome
//! indistinguishable. Run with:
//!   WHISPERPILOT_TEST_AUDIO=<sample.wav> cargo test --manifest-path src-tauri/Cargo.toml --test wp84_callback_regression -- --ignored --nocapture

use std::path::PathBuf;
use whisper_rs::{WhisperContext, WhisperContextParameters};

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

/// A CPU-decode context over the production model for a repeatable stress run.
fn cpu_context() -> Option<WhisperContext> {
    let model = app_support_dir()
        .join("models")
        .join("ggml-large-v3-turbo-q8_0.bin");
    let mut params = WhisperContextParameters::default();
    params.use_gpu(false);
    params.flash_attn(true);
    WhisperContext::new_with_params(model.to_str()?, params).ok()
}

/// Five consecutive decodes without a Stop callback must all complete.
#[test]
#[ignore]
fn decode_without_stop_callback_completes() {
    let (Some(audio), Some(ctx)) = (sample_path(), cpu_context()) else {
        eprintln!("SKIP: no sample audio or model (set WHISPERPILOT_TEST_AUDIO)");
        return;
    };
    let samples = whisperpilot_lib::audio::load_samples(&audio).expect("samples");

    for run in 1..=5 {
        let result = whisperpilot_lib::transcribe::transcribe(&ctx, &samples);
        assert!(
            result.is_ok(),
            "run {run}/5 failed without a Stop callback: {result:?}"
        );
    }
}
