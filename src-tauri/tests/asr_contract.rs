use whisperpilot_lib::asr::{
    resolve_selection, AsrEngine, AsrLanguage, AsrMode, AsrRuntime, DEFAULT_ASR_MODEL_ID,
    QWEN3_ASR_17_GGUF_MODEL_ID,
};

#[test]
fn legacy_default_resolves_to_whisper_for_every_existing_mode() {
    for mode in [AsrMode::Meeting, AsrMode::Streaming, AsrMode::Recorder] {
        let selection = resolve_selection(DEFAULT_ASR_MODEL_ID, mode, AsrLanguage::Auto).unwrap();
        assert_eq!(selection.engine, AsrEngine::Whisper);
        assert!(selection.capabilities.timestamps);
    }
}

#[test]
fn qwen_17_gguf_resolves_for_all_modes_with_automatic_language_detection() {
    for mode in [AsrMode::Meeting, AsrMode::Streaming, AsrMode::Recorder] {
        let selection =
            resolve_selection(QWEN3_ASR_17_GGUF_MODEL_ID, mode, AsrLanguage::Auto).unwrap();
        assert_eq!(selection.engine, AsrEngine::Qwen3Asr);
        assert_eq!(selection.runtime, AsrRuntime::LlamaCppMtmd);
        assert!(selection.capabilities.streaming);
        assert!(selection.capabilities.language_detection);
        assert!(selection.capabilities.mixed_language);
        assert!(!selection.capabilities.timestamps);
    }
}

#[test]
fn removed_or_unknown_models_never_fall_back_by_catalog_order() {
    for id in ["qwen3-asr-0.6b", "unknown-asr"] {
        assert!(resolve_selection(id, AsrMode::Recorder, AsrLanguage::Auto).is_err());
    }
}
