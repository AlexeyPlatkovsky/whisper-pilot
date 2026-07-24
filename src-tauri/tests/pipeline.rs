//! End-to-end pipeline check: file → ffmpeg → whisper → segments.
//!
//! Ignored by default: needs the whisper model on disk and ffmpeg on PATH.
//! Run with:
//!   cargo test --manifest-path src-tauri/Cargo.toml --test pipeline -- --ignored --nocapture

use std::path::PathBuf;

/// A real audio file to exercise the pipeline, in any language. Reuses a
/// VoicePilot fixture; override with WHISPERPILOT_TEST_AUDIO.
///
/// The English-specific regression below needs English speech, so it reads
/// WHISPERPILOT_TEST_AUDIO_EN instead and skips when that is unset — one
/// variable cannot serve both a language-agnostic and a language-asserting
/// test.
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

    // No language is passed in — auto-detection is the only mode.
    let result =
        whisperpilot_lib::transcribe::transcribe_file(&ctx, &audio).expect("transcription");
    let segments = &result.segments;

    eprintln!("detected language: {}", result.language);
    assert!(!segments.is_empty(), "expected at least one segment");
    for s in segments {
        eprintln!("[{:>6}–{:>6}ms] {}", s.start_ms, s.end_ms, s.text);
    }
    // Timestamps must be ordered and non-degenerate.
    assert!(segments.iter().all(|s| s.end_ms >= s.start_ms));
    // Whisper resolved a real language rather than falling through.
    assert_ne!(
        result.language,
        whisperpilot_lib::transcribe::UNDETECTED_LANGUAGE
    );
}

/// The WP-20 regression. Forcing Russian on English speech made whisper emit
/// one hallucinated subtitle credit per fixed 30s analysis window instead of a
/// transcript. Auto-detection must name the language and segment naturally.
///
/// Needs an English recording; point WHISPERPILOT_TEST_AUDIO_EN at one.
#[test]
#[ignore]
fn auto_detects_english_speech_instead_of_forcing_russian() {
    let Ok(audio) = std::env::var("WHISPERPILOT_TEST_AUDIO_EN").map(PathBuf::from) else {
        eprintln!("SKIP: no English sample audio (set WHISPERPILOT_TEST_AUDIO_EN)");
        return;
    };

    let ctx = match whisperpilot_lib::transcribe::load_model(&app_support_dir()) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("SKIP: model unavailable: {e}");
            return;
        }
    };

    let result =
        whisperpilot_lib::transcribe::transcribe_file(&ctx, &audio).expect("transcription");

    eprintln!("detected language: {}", result.language);
    assert_eq!(
        result.language, "en",
        "English speech must detect as English"
    );

    // Asserted before anything else: every check below this line is vacuously
    // true on an empty transcript. whisper.cpp returns from
    // whisper_full_with_state the moment it has detected the language if
    // detect_language is set, which yields Ok with zero segments and would let
    // a silently empty result pass for a fix.
    assert!(
        !result.segments.is_empty(),
        "auto-detection must still decode a transcript, not stop at the language"
    );

    // A segment spanning a whole 30s window is the signature of a collapsed
    // decode: whisper emits one per window with no internal timestamps.
    let degenerate: Vec<_> = result
        .segments
        .iter()
        .filter(|s| s.end_ms - s.start_ms >= 29_900)
        .collect();
    assert!(
        degenerate.is_empty(),
        "segments spanning a full 30s analysis window indicate a collapsed decode: {degenerate:?}"
    );
    // The known hallucination must not appear at all.
    assert!(
        !result
            .segments
            .iter()
            .any(|s| s.text.contains("Субтитры сделал")),
        "hallucinated Russian subtitle credit present in an English transcript"
    );
}
