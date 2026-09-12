#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::sync::Arc;
use whisperpilot_lib::asr::AsrLanguage;
use whisperpilot_lib::streaming_session::{QwenSessionDecoder, SessionDecoder};

/// Opt-in Apple Silicon gate for the exact qualified Qwen3-ASR bundle.
///
/// QWEN3_ASR_MODEL_DIR must contain model.safetensors, vocab.json and
/// merges.txt. QWEN3_ASR_TEST_WAV must be mono 16 kHz Russian speech.
#[test]
#[ignore = "requires the real Qwen3-ASR bundle and fixed speech corpus"]
fn qwen3_asr_06_recorder_window_decodes_real_russian_audio() {
    let model_dir = std::env::var("QWEN3_ASR_MODEL_DIR").expect("QWEN3_ASR_MODEL_DIR");
    let wav = PathBuf::from(std::env::var("QWEN3_ASR_TEST_WAV").expect("QWEN3_ASR_TEST_WAV"));
    let model = qwen_asr::context::QwenModel::load(&model_dir).expect("load Qwen3-ASR model");
    let samples =
        qwen_asr::audio::load_wav(wav.to_str().expect("UTF-8 WAV path")).expect("read 16 kHz WAV");
    let mut decoder = QwenSessionDecoder::new(Arc::clone(&model), AsrLanguage::Russian)
        .expect("create Recorder decoder");

    let transcription = decoder.decode_window(&samples).expect("decode speech");

    assert_eq!(transcription.language, "ru");
    assert!(!transcription.segments.is_empty());
    assert!(transcription.segments.iter().any(|segment| {
        let text = segment.text.to_lowercase();
        text.contains("локальн") && text.contains("расшифров")
    }));
}
