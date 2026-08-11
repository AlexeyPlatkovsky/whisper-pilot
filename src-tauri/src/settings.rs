//! Local key-value settings store: theme, ui_language, and the active
//! transcription model, persisted as JSON in the app support directory and
//! applied immediately and across restarts (F005-R2, F005-T1).

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
const NONE_DIARIZATION_MODEL: &str = "none";
const DEFAULT_EXPORT_FILE_TYPE: &str = "plain_text";

fn default_active_model_diarization() -> String {
    NONE_DIARIZATION_MODEL.to_string()
}

fn default_export_file_type() -> String {
    DEFAULT_EXPORT_FILE_TYPE.to_string()
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
    /// notes.
    #[serde(default = "default_export_file_type")]
    pub export_file_type: String,
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
        }
    }
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
}
