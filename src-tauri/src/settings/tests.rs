use super::*;
use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn concurrent_updates_to_different_keys_preserve_both_values() {
    // Each pair starts from the same on-disk snapshot. Without a single
    // read-modify-write gate, one writer can overwrite the other one's
    // independently valid change with that stale snapshot.
    for _ in 0..64 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let barrier = Arc::new(Barrier::new(2));

        thread::scope(|scope| {
            let theme_barrier = Arc::clone(&barrier);
            scope.spawn(move || {
                theme_barrier.wait();
                set_setting(path, KEY_THEME, "dark").unwrap();
            });
            let export_barrier = Arc::clone(&barrier);
            scope.spawn(move || {
                export_barrier.wait();
                set_setting(path, KEY_EXPORT_FILE_TYPE, "markdown").unwrap();
            });
        });

        let settings = get_settings(dir.path());
        assert_eq!(settings.theme, "dark");
        assert_eq!(settings.export_file_type, "markdown");
    }
}

#[test]
fn get_settings_defaults_status_colors_to_none() {
    let dir = tempfile::tempdir().unwrap();

    assert_eq!(get_settings(dir.path()).status_colors, None);
}

#[test]
fn set_setting_persists_status_colors_and_is_readable_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mapping = r##"{"ready":"#112233","error":"#B82B2F"}"##;

    set_setting(dir.path(), KEY_STATUS_COLORS, mapping).unwrap();
    let settings = get_settings(dir.path());

    assert_eq!(settings.status_colors, Some(mapping.to_string()));
}

#[test]
fn set_setting_rejects_status_colors_that_is_not_a_json_object() {
    let dir = tempfile::tempdir().unwrap();

    // EP: invalid partition — syntactically bad JSON, and valid JSON of
    // the wrong kind (array, bare string).
    for bad in ["not json", "[\"#112233\"]", "\"#112233\""] {
        let err = set_setting(dir.path(), KEY_STATUS_COLORS, bad).unwrap_err();
        assert!(matches!(err, AppError::InvalidSetting(_)), "input: {bad}");
    }
    assert_eq!(get_settings(dir.path()).status_colors, None);
}

#[test]
fn set_setting_rejects_status_colors_with_a_non_opaque_hex_value() {
    let dir = tempfile::tempdir().unwrap();
    set_setting(dir.path(), KEY_STATUS_COLORS, r##"{"ready":"#112233"}"##).unwrap();

    // EP: invalid value partition — shorthand, alpha-bearing, missing '#',
    // non-hex digits, empty. Every rejection must leave the prior valid
    // write untouched.
    for bad_value in ["#123", "#11223344", "112233", "#GGGGGG", ""] {
        let payload = format!(r#"{{"ready":"{bad_value}"}}"#);
        let err = set_setting(dir.path(), KEY_STATUS_COLORS, &payload).unwrap_err();
        assert!(
            matches!(err, AppError::InvalidSetting(_)),
            "value: {bad_value}"
        );
    }
    // The prior valid write must survive the rejected writes.
    assert_eq!(
        get_settings(dir.path()).status_colors,
        Some(r##"{"ready":"#112233"}"##.to_string())
    );
}

#[test]
fn set_setting_rejects_status_colors_with_a_non_string_entry() {
    let dir = tempfile::tempdir().unwrap();

    // EP: wrong-kind partition — an entry whose value is not a string.
    let err = set_setting(dir.path(), KEY_STATUS_COLORS, r#"{"ready":123}"#).unwrap_err();

    assert!(matches!(err, AppError::InvalidSetting(_)));
    assert_eq!(get_settings(dir.path()).status_colors, None);
}

#[test]
fn get_settings_returns_defaults_when_no_store_file_exists() {
    let dir = tempfile::tempdir().unwrap();

    let settings = get_settings(dir.path());

    assert_eq!(settings.theme, "system");
    assert_eq!(settings.ui_language, "en");
    assert_eq!(settings.active_model_transcription, None);
}

#[test]
fn qwen_17_gguf_is_the_single_persisted_asr_selection() {
    let dir = tempfile::tempdir().unwrap();
    let id = crate::asr::QWEN3_ASR_17_GGUF_MODEL_ID;

    let settings = set_setting(dir.path(), KEY_ACTIVE_MODEL_TRANSCRIPTION, id).unwrap();

    assert_eq!(settings.active_model_transcription.as_deref(), Some(id));
}

#[test]
fn removed_llm_selections_are_safely_cleared_when_old_settings_are_read() {
    for removed_id in ["qwen2.5-3b-q3km", "qwen3-4b-q3kl"] {
        let dir = tempfile::tempdir().unwrap();
        let old = format!(
            r#"{{"theme":"system","ui_language":"en","active_model_diarization":"none","active_model_llm":"{removed_id}","export_file_type":"plain_text"}}"#
        );
        std::fs::write(dir.path().join(FILE_NAME), old).unwrap();

        assert_eq!(get_settings(dir.path()).active_model_llm, None);
    }
}

#[test]
fn get_settings_falls_back_to_defaults_on_a_corrupt_store_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(FILE_NAME), "not json").unwrap();

    let settings = get_settings(dir.path());

    assert_eq!(settings, Settings::default());
}

#[test]
fn set_setting_persists_theme_and_is_readable_after_restart() {
    let dir = tempfile::tempdir().unwrap();

    set_setting(dir.path(), KEY_THEME, "dark").unwrap();
    // A fresh get_settings call simulates reading after an app restart —
    // nothing is cached in memory between calls.
    let settings = get_settings(dir.path());

    assert_eq!(settings.theme, "dark");
}

#[test]
fn set_setting_persists_active_model_transcription() {
    let dir = tempfile::tempdir().unwrap();

    let settings =
        set_setting(dir.path(), KEY_ACTIVE_MODEL_TRANSCRIPTION, "transcription").unwrap();

    assert_eq!(
        settings.active_model_transcription,
        Some("transcription".to_string())
    );
}

#[test]
fn export_file_type_defaults_to_plain_text() {
    let dir = tempfile::tempdir().unwrap();

    assert_eq!(get_settings(dir.path()).export_file_type, "plain_text");
}

#[test]
fn set_setting_persists_export_file_type_and_is_readable_after_restart() {
    let dir = tempfile::tempdir().unwrap();

    set_setting(dir.path(), KEY_EXPORT_FILE_TYPE, "markdown").unwrap();
    let settings = get_settings(dir.path());

    assert_eq!(settings.export_file_type, "markdown");
}

#[test]
fn set_setting_rejects_invalid_export_file_type_and_leaves_store_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    set_setting(dir.path(), KEY_EXPORT_FILE_TYPE, "markdown").unwrap();

    let err = set_setting(dir.path(), KEY_EXPORT_FILE_TYPE, "pdf").unwrap_err();

    assert!(matches!(err, AppError::InvalidSetting(_)));
    assert_eq!(get_settings(dir.path()).export_file_type, "markdown");
}

#[test]
fn set_setting_rejects_unknown_key_and_leaves_store_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    set_setting(dir.path(), KEY_THEME, "dark").unwrap();

    let err = set_setting(dir.path(), "not_a_real_key", "x").unwrap_err();

    assert!(matches!(err, AppError::InvalidSetting(_)));
    // The prior valid write must survive a later rejected write.
    assert_eq!(get_settings(dir.path()).theme, "dark");
}

#[test]
fn set_setting_rejects_invalid_theme_value_and_leaves_store_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    set_setting(dir.path(), KEY_THEME, "dark").unwrap();

    let err = set_setting(dir.path(), KEY_THEME, "purple").unwrap_err();

    assert!(matches!(err, AppError::InvalidSetting(_)));
    assert_eq!(get_settings(dir.path()).theme, "dark");
}

#[test]
fn set_setting_rejects_unsupported_ui_language_in_beta_and_leaves_store_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    set_setting(dir.path(), KEY_UI_LANGUAGE, "en").unwrap();

    let err = set_setting(dir.path(), KEY_UI_LANGUAGE, "ru").unwrap_err();

    assert!(matches!(err, AppError::InvalidSetting(_)));
    assert_eq!(get_settings(dir.path()).ui_language, "en");
}

#[test]
fn set_setting_rejects_empty_active_model_transcription_and_leaves_store_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    set_setting(dir.path(), KEY_ACTIVE_MODEL_TRANSCRIPTION, "transcription").unwrap();

    let empty = set_setting(dir.path(), KEY_ACTIVE_MODEL_TRANSCRIPTION, "").unwrap_err();
    let whitespace = set_setting(dir.path(), KEY_ACTIVE_MODEL_TRANSCRIPTION, "   ").unwrap_err();

    assert!(matches!(empty, AppError::InvalidSetting(_)));
    assert!(matches!(whitespace, AppError::InvalidSetting(_)));
    assert_eq!(
        get_settings(dir.path()).active_model_transcription,
        Some("transcription".to_string())
    );
}

#[test]
fn set_setting_rejects_model_id_not_in_catalog_and_leaves_store_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    set_setting(dir.path(), KEY_ACTIVE_MODEL_TRANSCRIPTION, "transcription").unwrap();

    let err = set_setting(dir.path(), KEY_ACTIVE_MODEL_TRANSCRIPTION, "whisper-base").unwrap_err();

    assert!(matches!(err, AppError::InvalidSetting(_)));
    assert_eq!(
        get_settings(dir.path()).active_model_transcription,
        Some("transcription".to_string())
    );
}

#[test]
fn get_settings_defaults_active_model_diarization_to_none() {
    let dir = tempfile::tempdir().unwrap();

    let settings = get_settings(dir.path());

    assert_eq!(settings.active_model_diarization, "none");
}

#[test]
fn set_setting_persists_active_model_diarization_as_none() {
    let dir = tempfile::tempdir().unwrap();
    set_setting(dir.path(), KEY_ACTIVE_MODEL_DIARIZATION, "campplus").unwrap();

    let settings = set_setting(dir.path(), KEY_ACTIVE_MODEL_DIARIZATION, "none").unwrap();

    assert_eq!(settings.active_model_diarization, "none");
}

#[test]
fn set_setting_persists_active_model_diarization_as_a_known_variant() {
    let dir = tempfile::tempdir().unwrap();

    let settings = set_setting(dir.path(), KEY_ACTIVE_MODEL_DIARIZATION, "titanet-large").unwrap();

    assert_eq!(settings.active_model_diarization, "titanet-large");
}

#[test]
fn set_setting_rejects_active_model_diarization_value_not_a_known_variant_and_leaves_store_unchanged(
) {
    let dir = tempfile::tempdir().unwrap();
    set_setting(dir.path(), KEY_ACTIVE_MODEL_DIARIZATION, "campplus").unwrap();

    let err = set_setting(
        dir.path(),
        KEY_ACTIVE_MODEL_DIARIZATION,
        "not-a-real-variant",
    )
    .unwrap_err();

    assert!(matches!(err, AppError::InvalidSetting(_)));
    assert_eq!(
        get_settings(dir.path()).active_model_diarization,
        "campplus"
    );
}

// WP-96: MFU panel visibility, one independent boolean key per screen.

#[test]
fn get_settings_defaults_mfu_panel_meeting_to_true() {
    let dir = tempfile::tempdir().unwrap();

    assert!(get_settings(dir.path()).mfu_panel_meeting);
}

#[test]
fn get_settings_defaults_mfu_panel_streaming_to_true() {
    let dir = tempfile::tempdir().unwrap();

    assert!(get_settings(dir.path()).mfu_panel_streaming);
}

#[test]
fn set_setting_persists_mfu_panel_meeting_and_is_readable_after_restart() {
    let dir = tempfile::tempdir().unwrap();

    set_setting(dir.path(), KEY_MFU_PANEL_MEETING, "false").unwrap();
    let settings = get_settings(dir.path());

    assert!(!settings.mfu_panel_meeting);
    // The two screens' keys are independent (S-3): changing Meeting's
    // must not disturb Streaming's default.
    assert!(settings.mfu_panel_streaming);
}

#[test]
fn set_setting_persists_mfu_panel_streaming_independently_of_meeting() {
    let dir = tempfile::tempdir().unwrap();

    set_setting(dir.path(), KEY_MFU_PANEL_STREAMING, "false").unwrap();
    let settings = get_settings(dir.path());

    assert!(!settings.mfu_panel_streaming);
    assert!(settings.mfu_panel_meeting);
}

#[test]
fn set_setting_toggling_mfu_panel_meeting_back_to_true_is_readable_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    set_setting(dir.path(), KEY_MFU_PANEL_MEETING, "false").unwrap();

    set_setting(dir.path(), KEY_MFU_PANEL_MEETING, "true").unwrap();

    assert!(get_settings(dir.path()).mfu_panel_meeting);
}

#[test]
fn set_setting_rejects_invalid_mfu_panel_meeting_value_and_leaves_store_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    set_setting(dir.path(), KEY_MFU_PANEL_MEETING, "false").unwrap();

    // EP: invalid value partition — only the literal strings "true"/
    // "false" are accepted.
    for bad in ["yes", "1", "TRUE", "False", "", "no"] {
        let err = set_setting(dir.path(), KEY_MFU_PANEL_MEETING, bad).unwrap_err();
        assert!(matches!(err, AppError::InvalidSetting(_)), "value: {bad}");
    }
    assert!(!get_settings(dir.path()).mfu_panel_meeting);
}

#[test]
fn set_setting_rejects_invalid_mfu_panel_streaming_value_and_leaves_store_unchanged() {
    let dir = tempfile::tempdir().unwrap();

    let err = set_setting(dir.path(), KEY_MFU_PANEL_STREAMING, "off").unwrap_err();

    assert!(matches!(err, AppError::InvalidSetting(_)));
    assert!(get_settings(dir.path()).mfu_panel_streaming);
}

#[test]
fn get_settings_defaults_mfu_panel_keys_to_true_for_a_pre_wp90_store_file() {
    let dir = tempfile::tempdir().unwrap();
    // A settings file written before mfu_panel_meeting / mfu_panel_streaming
    // existed — both keys must default to true rather than fail to parse.
    let pre_wp90_json = r#"{"theme":"system","ui_language":"en","active_model_diarization":"none","export_file_type":"plain_text"}"#;
    std::fs::write(dir.path().join(FILE_NAME), pre_wp90_json).unwrap();

    let settings = get_settings(dir.path());

    assert!(settings.mfu_panel_meeting);
    assert!(settings.mfu_panel_streaming);
}
