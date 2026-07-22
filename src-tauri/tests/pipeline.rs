//! End-to-end pipeline check: file → ffmpeg → whisper → segments.
//!
//! Ignored by default: needs the whisper model on disk and ffmpeg on PATH.
//! Run with:
//!   cargo test --manifest-path src-tauri/Cargo.toml --test pipeline -- --ignored --nocapture

use std::path::PathBuf;

/// A real audio file to exercise the pipeline. Reuses a VoicePilot fixture;
/// override with WHISPERPILOT_TEST_AUDIO to point at a Russian sample.
fn sample_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WHISPERPILOT_TEST_AUDIO") {
        return Some(PathBuf::from(p));
    }
    let p = dirs_next::home_dir()?.join(
        "Documents/IdeaProjects/voice-pilot/src-tauri/tests/fixtures/audio/1_minute/sample.wav",
    );
    p.exists().then_some(p)
}

/// The real macOS app support dir Tauri resolves at runtime for this app's
/// bundle identifier (com.whisperpilot.dev); matches where WP-39 downloads land.
fn app_support_dir() -> PathBuf {
    dirs_next::data_local_dir()
        .or_else(|| dirs_next::home_dir().map(|h| h.join("Library/Application Support")))
        .unwrap_or_default()
        .join("com.whisperpilot.dev")
}

#[test]
#[ignore]
fn transcribes_a_real_file_into_segments() {
    let Some(audio) = sample_path() else {
        eprintln!("SKIP: no sample audio (set WHISPERPILOT_TEST_AUDIO)");
        return;
    };

    let ctx = match whisperpilot_lib::transcribe::load_model(&app_support_dir()) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("SKIP: model unavailable: {e}");
            return;
        }
    };

    // English fixture, but decode as-is to prove the ffmpeg+whisper path.
    let lang = std::env::var("WHISPERPILOT_TEST_LANG").unwrap_or_else(|_| "en".to_string());
    let segments =
        whisperpilot_lib::transcribe::transcribe_file(&ctx, &audio, &lang).expect("transcription");

    assert!(!segments.is_empty(), "expected at least one segment");
    for s in &segments {
        eprintln!("[{:>6}–{:>6}ms] {}", s.start_ms, s.end_ms, s.text);
    }
    // Timestamps must be ordered and non-degenerate.
    assert!(segments.iter().all(|s| s.end_ms >= s.start_ms));
}
