use whisperpilot_lib::asr::{
    resolve_selection, AsrEngine, AsrLanguage, AsrMode, DEFAULT_ASR_MODEL_ID, QWEN3_ASR_06_MODEL_ID,
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
fn qwen_is_recorder_only_and_requires_an_explicit_language() {
    let selection = resolve_selection(
        QWEN3_ASR_06_MODEL_ID,
        AsrMode::Recorder,
        AsrLanguage::Russian,
    )
    .unwrap();
    assert_eq!(selection.engine, AsrEngine::Qwen3Asr);
    assert!(!selection.capabilities.timestamps);

    assert!(
        resolve_selection(QWEN3_ASR_06_MODEL_ID, AsrMode::Recorder, AsrLanguage::Auto,).is_err()
    );
    assert!(resolve_selection(
        QWEN3_ASR_06_MODEL_ID,
        AsrMode::Streaming,
        AsrLanguage::Russian,
    )
    .is_err());
}

#[test]
fn unknown_model_never_falls_back_by_catalog_order() {
    assert!(resolve_selection("unknown-asr", AsrMode::Recorder, AsrLanguage::English,).is_err());
}
