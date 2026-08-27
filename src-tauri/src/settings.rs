//! Local key-value settings store: theme, ui_language, the active
//! transcription model, export file type, the configurable status colors
//! (WP-88), and per-screen MFU panel visibility (WP-90), persisted as JSON in
//! the app support directory and applied immediately and across restarts
//! (F005-R2, F005-T1).

use crate::error::{AppError, Result};
use crate::models::CATALOG;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "settings.json";

const KEY_THEME: &str = "theme";
const KEY_UI_LANGUAGE: &str = "ui_language";
// The dotted key namespace (`active_model.<task>`) is a set_setting() argument
// only; it intentionally does not match the struct field name below, since a
// future task-scoped key (e.g. `active_model.diarization`) would not map
// cleanly to a single Rust field.
const KEY_ACTIVE_MODEL_TRANSCRIPTION: &str = "active_model.transcription";
const KEY_ACTIVE_MODEL_DIARIZATION: &str = "active_model.diarization";
const KEY_ACTIVE_MODEL_LLM: &str = "active_model.llm";
const KEY_EXPORT_FILE_TYPE: &str = "export_file_type";
// WP-88: the user-configured per-status color mapping, stored as one JSON
// object string (`{"ready":"#112233",…}`) so the front-end-owned status key
// set can grow without a settings-store schema change.
const KEY_STATUS_COLORS: &str = "status_colors";
// WP-90: view-only visibility of each screen's MFU (summary) panel, one
// independent boolean key per screen so restoring one on launch never
// disturbs the other.
const KEY_MFU_PANEL_MEETING: &str = "mfu_panel_meeting";
const KEY_MFU_PANEL_STREAMING: &str = "mfu_panel_streaming";
const NONE_DIARIZATION_MODEL: &str = "none";
const DEFAULT_EXPORT_FILE_TYPE: &str = "plain_text";

fn default_active_model_diarization() -> String {
    NONE_DIARIZATION_MODEL.to_string()
}

fn default_export_file_type() -> String {
    DEFAULT_EXPORT_FILE_TYPE.to_string()
}

fn default_true() -> bool {
    true
}

/// Strict "true"/"false" only (WP-90 non-goal: no other truthy/falsy spelling).
fn parse_bool_setting(key: &str, value: &str) -> Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(AppError::InvalidSetting(format!(
            "{key} must be \"true\" or \"false\", got {other}"
        ))),
    }
}

/// All persisted settings, always fully populated with defaults for unset keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String,
    pub ui_language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_model_transcription: Option<String>,
    #[serde(default = "default_active_model_diarization")]
    pub active_model_diarization: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_model_llm: Option<String>,
    /// `"plain_text"` or `"markdown"` (WP-15): governs how export-to-file and
    /// the header label's clipboard copy render a meeting's transcript and
    /// mfu.
    #[serde(default = "default_export_file_type")]
    pub export_file_type: String,
    /// WP-88: JSON object mapping each configurable status key to an opaque
    /// `#RRGGBB` color; `None` before the setting is first saved (startup
    /// then uses the built-in mapping).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_colors: Option<String>,
    /// WP-90: whether the Meeting screen's MFU panel is shown. View-only —
    /// never gates Craft MFU or any other action. Defaults ON.
    #[serde(default = "default_true")]
    pub mfu_panel_meeting: bool,
    /// WP-90: same as `mfu_panel_meeting`, for the Streaming screen. Kept
    /// independent so restoring one on launch never disturbs the other.
    #[serde(default = "default_true")]
    pub mfu_panel_streaming: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "system".to_string(),
            ui_language: "en".to_string(),
            active_model_transcription: None,
            active_model_diarization: default_active_model_diarization(),
            active_model_llm: None,
            export_file_type: default_export_file_type(),
            status_colors: None,
            mfu_panel_meeting: default_true(),
            mfu_panel_streaming: default_true(),
        }
    }
}

/// Opaque six-digit `#RRGGBB` only — shorthand and alpha-bearing values are
/// rejected (WP-88 non-goals).
fn is_opaque_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

fn settings_path(app_support_dir: &Path) -> PathBuf {
    app_support_dir.join(FILE_NAME)
}

/// Read all settings, falling back to defaults when no store file exists yet
/// or the file cannot be parsed.
pub fn get_settings(app_support_dir: &Path) -> Settings {
    std::fs::read_to_string(settings_path(app_support_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Update one known setting and persist the full store; rejects an unknown
/// key or an invalid value without touching the file.
pub fn set_setting(app_support_dir: &Path, key: &str, value: &str) -> Result<Settings> {
    let mut settings = get_settings(app_support_dir);

    match key {
        KEY_THEME => {
            if !matches!(value, "light" | "dark" | "system") {
                return Err(AppError::InvalidSetting(format!(
                    "theme must be light, dark, or system, got {value}"
                )));
            }
            settings.theme = value.to_string();
        }
        KEY_UI_LANGUAGE => {
            if value != "en" {
                return Err(AppError::InvalidSetting(format!(
                    "ui_language must be en in beta, got {value}"
                )));
            }
            settings.ui_language = value.to_string();
        }
        KEY_ACTIVE_MODEL_TRANSCRIPTION => {
            if value.trim().is_empty() {
                return Err(AppError::InvalidSetting(
                    "active_model.transcription must not be empty".to_string(),
                ));
            }
            if !CATALOG.iter().any(|e| e.id == value) {
                return Err(AppError::InvalidSetting(format!(
                    "unknown model id: {value}",
                )));
            }
            settings.active_model_transcription = Some(value.to_string());
        }
        KEY_ACTIVE_MODEL_DIARIZATION => {
            let is_known_variant = value == NONE_DIARIZATION_MODEL
                || CATALOG
                    .iter()
                    .any(|e| e.assets.iter().any(|a| a.variant_id == Some(value)));
            if !is_known_variant {
                return Err(AppError::InvalidSetting(format!(
                    "unknown diarization model id: {value}",
                )));
            }
            settings.active_model_diarization = value.to_string();
        }
        KEY_ACTIVE_MODEL_LLM => {
            if value.trim().is_empty() {
                settings.active_model_llm = None;
            } else if !CATALOG.iter().any(|e| e.id == value) {
                return Err(AppError::InvalidSetting(format!(
                    "unknown model id: {value}",
                )));
            } else {
                settings.active_model_llm = Some(value.to_string());
            }
        }
        KEY_EXPORT_FILE_TYPE => {
            if !matches!(value, "plain_text" | "markdown") {
                return Err(AppError::InvalidSetting(format!(
                    "export_file_type must be plain_text or markdown, got {value}"
                )));
            }
            settings.export_file_type = value.to_string();
        }
        KEY_STATUS_COLORS => {
            let parsed: serde_json::Value = serde_json::from_str(value).map_err(|_| {
                AppError::InvalidSetting(
                    "status_colors must be a JSON object of #RRGGBB colors".to_string(),
                )
            })?;
            let map = parsed.as_object().ok_or_else(|| {
                AppError::InvalidSetting(
                    "status_colors must be a JSON object of #RRGGBB colors".to_string(),
                )
            })?;
            for (status, color) in map {
                let color = color.as_str().ok_or_else(|| {
                    AppError::InvalidSetting(format!(
                        "status_colors[{status}] must be a #RRGGBB string"
                    ))
                })?;
                if !is_opaque_hex_color(color) {
                    return Err(AppError::InvalidSetting(format!(
                        "status_colors[{status}] must be an opaque six-digit hex color, got {color}"
                    )));
                }
            }
            settings.status_colors = Some(value.to_string());
        }
        KEY_MFU_PANEL_MEETING => {
            settings.mfu_panel_meeting = parse_bool_setting(KEY_MFU_PANEL_MEETING, value)?;
        }
        KEY_MFU_PANEL_STREAMING => {
            settings.mfu_panel_streaming = parse_bool_setting(KEY_MFU_PANEL_STREAMING, value)?;
        }
        other => {
            return Err(AppError::InvalidSetting(format!(
                "unknown setting key: {other}"
            )));
        }
    }

    write_settings(app_support_dir, &settings)?;
    Ok(settings)
}

fn write_settings(app_support_dir: &Path, settings: &Settings) -> Result<()> {
    std::fs::create_dir_all(app_support_dir)?;
    let json = serde_json::to_string_pretty(settings).map_err(|e| AppError::Io(e.to_string()))?;
    std::fs::write(settings_path(app_support_dir), json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let whitespace =
            set_setting(dir.path(), KEY_ACTIVE_MODEL_TRANSCRIPTION, "   ").unwrap_err();

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

        let err =
            set_setting(dir.path(), KEY_ACTIVE_MODEL_TRANSCRIPTION, "whisper-base").unwrap_err();

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

        let settings =
            set_setting(dir.path(), KEY_ACTIVE_MODEL_DIARIZATION, "titanet-large").unwrap();

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

    // WP-90: MFU panel visibility, one independent boolean key per screen.

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
}
