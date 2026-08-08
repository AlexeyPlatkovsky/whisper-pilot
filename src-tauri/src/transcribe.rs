//! Offline, full-file transcription with whisper-rs on Metal. The whole
//! file is decoded in one pass with beam search and no real-time
//! constraint, so accuracy is the only priority.
//!
//! The language is always auto-detected — there is no way to force one; see
//! ADR-012 for why forcing one is actively harmful, not just less accurate.
//! Whisper decides the language itself, and what it decided is reported
//! back so the caller can record it.

use crate::error::{AppError, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

/// The language of audio that has not been decoded yet, and the fallback when
/// whisper reports an id we cannot name. Doubles as the value handed to whisper
/// to request detection, which is the same token whisper.cpp itself uses.
pub const UNDETECTED_LANGUAGE: &str = "auto";

/// Name the language whisper reported on its decoder state.
///
/// `whisper_lang_str` returns null for an id outside its table, and the state
/// reports `-1` when it never resolved one; neither may panic, because a
/// finished transcript still has to be storable.
pub fn detected_language(lang_id: i32) -> String {
    whisper_rs::get_lang_str(lang_id)
        .unwrap_or(UNDETECTED_LANGUAGE)
        .to_string()
}

/// How a transcribing run is configured, kept as plain data so it can be
/// asserted directly. whisper-rs's `FullParams` exposes no getters, so without
/// this seam the one setting that decides whether a run decodes at all is
/// unreachable from a test — which is exactly where WP-20's first fix attempt
/// went wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeSettings {
    /// Always the auto-detect token. There is no way to force a language: doing
    /// so on audio in another language collapses the decode into one
    /// hallucinated line per 30-second window.
    pub language: &'static str,
    /// Must stay `false`. `whisper_full_with_state` returns as soon as it has
    /// detected the language when this is set, producing an empty transcript.
    pub detect_language_only: bool,
    pub translate: bool,
}

/// The one configuration every transcribing run uses.
pub fn decode_settings() -> DecodeSettings {
    DecodeSettings {
        language: UNDETECTED_LANGUAGE,
        detect_language_only: false,
        translate: false,
    }
}

/// A finished decode: the transcript and the language whisper chose for it.
#[derive(Debug)]
pub struct Transcription {
    pub segments: Vec<Segment>,
    /// The detected language code (`"en"`, `"ru"`, …), or
    /// [`UNDETECTED_LANGUAGE`] when whisper named none.
    pub language: String,
}

/// One transcript segment with its time span, in milliseconds from file start.
/// `speaker_id` is absent until diarization is wired in (WP-31); omitted from
/// the serialized JSON (not `null`) when `None`, so existing consumers see no
/// shape change.
#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_id: Option<i32>,
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
    resolve_model_path(
        app_support_dir,
        std::env::var("WHISPERPILOT_MODEL_PATH").ok(),
    )
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

/// Transcribe an entire file of 16 kHz mono samples into timestamped segments,
/// letting whisper detect the language.
///
/// Creates a fresh [`WhisperState`] for this run — the right granularity for
/// Meeting's one-shot whole-file decode. Streaming instead reuses one state
/// per session through [`transcribe_with_state`] (WP-82): each state owns a
/// full GPU backend plus its compute buffers, so one state per window would
/// be one backend init/free cycle per window.
///
/// `cancel` is polled by whisper's abort callback during decode (WP-19): once
/// set, whisper stops and this returns [`AppError::Cancelled`] instead of a
/// (partial) transcript, so a stopped run never produces a document.
///
/// `on_progress` (WP-58) is called with whisper's own 0-100 percent-complete
/// figure as decoding proceeds, on whichever thread runs this function (never
/// concurrently with itself, since whisper drives it synchronously).
pub fn transcribe(
    ctx: &WhisperContext,
    samples: &[f32],
    cancel: &Arc<AtomicBool>,
    on_progress: impl FnMut(i32) + 'static,
) -> Result<Transcription> {
    let mut state = ctx
        .create_state()
        .map_err(|e| AppError::Transcribe(e.to_string()))?;
    transcribe_with_state(&mut state, samples, cancel, on_progress)
}

/// [`transcribe`] with a caller-owned state. Reusing one state across calls
/// is upstream's own pattern (`whisper_full` reuses `ctx->state`; each call
/// clears its results, recomputes the mel spectrogram, and clears the
/// self-attention KV cache), so a sequential caller may keep one state for as
/// long as it likes.
pub fn transcribe_with_state(
    state: &mut WhisperState,
    samples: &[f32],
    cancel: &Arc<AtomicBool>,
    on_progress: impl FnMut(i32) + 'static,
) -> Result<Transcription> {
    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }

    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: -1.0,
    });
    params.set_n_threads(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4) as i32,
    );
    // Passing the auto token is what requests detection; there is deliberately
    // no way for a caller to substitute a fixed language.
    //
    // `set_detect_language` is NOT the equivalent switch whisper-rs's own docs
    // claim it is: it means "detect and stop", so setting it here would return
    // an empty transcript. It is spelled out in `DecodeSettings` and asserted
    // in a test so the mistake cannot be made again silently.
    let settings = decode_settings();
    params.set_language(Some(settings.language));
    params.set_detect_language(settings.detect_language_only);
    params.set_translate(settings.translate);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    // Offline: let whisper segment naturally, and lean on its fallbacks.
    params.set_temperature_inc(0.2);
    params.set_suppress_blank(true);
    let abort_flag = Arc::clone(cancel);
    params.set_abort_callback_safe(move || abort_flag.load(Ordering::Relaxed));
    params.set_progress_callback_safe(on_progress);

    let full_result = state.full(params, samples);
    // The abort callback stops whisper's decode loop, but whisper.cpp still
    // returns success with whatever partial output it had, not a distinct
    // error code — so cancellation is detected from our own flag, checked
    // before the decode's own Err (a real decode failure), not after it.
    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }
    full_result.map_err(|e| AppError::Transcribe(e.to_string()))?;

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
            speaker_id: None,
        });
    }

    Ok(Transcription {
        segments,
        // Read after the decode, so this is what whisper actually used rather
        // than what it was asked for.
        language: detected_language(state.full_lang_id_from_state()),
    })
}

/// Full path from a picked file to timestamped segments.
pub fn transcribe_file(ctx: &WhisperContext, input: &Path) -> Result<Transcription> {
    let samples = crate::audio::load_samples(input)?;
    transcribe(ctx, &samples, &Arc::new(AtomicBool::new(false)), |_| {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_omits_speaker_id_key_when_none() {
        let segment = Segment {
            start_ms: 0,
            end_ms: 1_000,
            text: "hello".to_string(),
            speaker_id: None,
        };

        let value = serde_json::to_value(&segment).unwrap();

        assert!(
            value.get("speaker_id").is_none(),
            "speaker_id must be omitted, not present as null, when None: {value:?}"
        );
    }

    #[test]
    fn segment_includes_speaker_id_when_some() {
        let segment = Segment {
            start_ms: 0,
            end_ms: 1_000,
            text: "hello".to_string(),
            speaker_id: Some(2),
        };

        let value = serde_json::to_value(&segment).unwrap();

        assert_eq!(value.get("speaker_id"), Some(&serde_json::json!(2)));
    }

    #[test]
    fn decode_settings_ask_whisper_to_detect_the_language() {
        let settings = decode_settings();

        assert_eq!(settings.language, "auto");
        assert!(!settings.translate);
    }

    #[test]
    fn decode_settings_never_stop_at_language_detection() {
        // whisper_full_with_state returns 0 the moment it has detected the
        // language when detect_language is set (whisper.cpp:6823), leaving
        // result_all empty. A transcribing run that set it would return Ok with
        // zero segments and persist a blank transcript as "finished".
        assert!(!decode_settings().detect_language_only);
    }

    #[test]
    fn detected_language_names_the_language_whisper_reports() {
        // Whisper's own id table: 0 is English, 4 is Russian.
        assert_eq!(detected_language(0), "en");
        assert_eq!(detected_language(4), "ru");
    }

    #[test]
    fn ep_detected_language_falls_back_to_undetected_for_an_id_it_cannot_name() {
        // EP: the two invalid-id classes — the unresolved sentinel the decoder
        // state reports when it never named a language (-1), and an id outside
        // whisper's table (9_999), for which whisper_lang_str returns null.
        // Neither may panic — a finished transcript must still be storable.
        assert_eq!(detected_language(-1), UNDETECTED_LANGUAGE);
        assert_eq!(detected_language(9_999), UNDETECTED_LANGUAGE);
    }

    #[test]
    fn undetected_language_is_the_auto_sentinel() {
        assert_eq!(UNDETECTED_LANGUAGE, "auto");
    }

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
            dir.path()
                .join("models")
                .join("ggml-large-v3-turbo-q8_0.bin")
        );
    }
}
