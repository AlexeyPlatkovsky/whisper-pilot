//! Local key-value settings store: theme, ui_language, the active
//! transcription model, export file type, the configurable status colors
//! (WP-88), and per-screen MFU panel visibility (WP-96), persisted as JSON in
//! the app support directory and applied immediately and across restarts
//! (F005-R2, F005-T1).

use crate::error::{AppError, Result};
use crate::models::CATALOG;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

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
// WP-96: view-only visibility of each screen's MFU (summary) panel, one
// independent boolean key per screen so restoring one on launch never
// disturbs the other.
const KEY_MFU_PANEL_MEETING: &str = "mfu_panel_meeting";
const KEY_MFU_PANEL_STREAMING: &str = "mfu_panel_streaming";
const KEY_CLOUD_PROVIDER: &str = "cloud_provider";
const KEY_RECORDER_SHORTCUT: &str = "recorder_shortcut";
const KEY_BUBBLE_ALWAYS_ON_TOP: &str = "bubble_always_on_top";
const NONE_DIARIZATION_MODEL: &str = "none";
const DEFAULT_EXPORT_FILE_TYPE: &str = "plain_text";
const DEFAULT_CLOUD_PROVIDER: &str = "deepgram";

// Settings updates are read-modify-write operations. A process-wide gate
// makes independently initiated UI updates linearizable, even when they
// target separate app-support directories in tests.
static SETTINGS_WRITE_GATE: OnceLock<Mutex<()>> = OnceLock::new();
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn settings_write_gate() -> &'static Mutex<()> {
    SETTINGS_WRITE_GATE.get_or_init(|| Mutex::new(()))
}

fn default_active_model_diarization() -> String {
    NONE_DIARIZATION_MODEL.to_string()
}

fn default_export_file_type() -> String {
    DEFAULT_EXPORT_FILE_TYPE.to_string()
}

fn default_true() -> bool {
    true
}

fn default_cloud_provider() -> String {
    DEFAULT_CLOUD_PROVIDER.to_string()
}

fn default_recorder_shortcut() -> String {
    crate::recorder_shortcut::DEFAULT_RECORDER_SHORTCUT.to_string()
}

/// Strict "true"/"false" only (WP-96 non-goal: no other truthy/falsy spelling).
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// WP-96: whether the Meeting screen's MFU panel is shown. View-only —
    /// never gates Craft MFU or any other action. Defaults ON.
    #[serde(default = "default_true")]
    pub mfu_panel_meeting: bool,
    /// WP-96: same as `mfu_panel_meeting`, for the Streaming screen. Kept
    /// independent so restoring one on launch never disturbs the other.
    #[serde(default = "default_true")]
    pub mfu_panel_streaming: bool,
    /// The selected provider identifier only. API-key material is held in
    /// macOS Keychain and never belongs in this JSON settings file.
    #[serde(default = "default_cloud_provider")]
    pub cloud_provider: String,
    /// Canonical user-facing accelerator. Platform registration is owned by
    /// Rust and retains the previous value if a replacement conflicts.
    #[serde(default = "default_recorder_shortcut")]
    pub recorder_shortcut: String,
    /// When enabled the 120 px Recorder bubble floats above normal windows
    /// and joins every macOS Space, including fullscreen Spaces.
    #[serde(default)]
    pub bubble_always_on_top: bool,
    /// Last bubble top-left in logical pixels. Both coordinates are optional
    /// so older settings files and interrupted first moves remain valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bubble_x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bubble_y: Option<f64>,
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
            cloud_provider: default_cloud_provider(),
            recorder_shortcut: default_recorder_shortcut(),
            bubble_always_on_top: false,
            bubble_x: None,
            bubble_y: None,
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
    let mut settings: Settings = std::fs::read_to_string(settings_path(app_support_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if settings.active_model_llm.as_deref().is_some_and(|id| {
        !CATALOG
            .iter()
            .any(|entry| entry.task == "llm" && entry.id == id)
    }) {
        settings.active_model_llm = None;
    }
    if settings
        .active_model_transcription
        .as_deref()
        .is_some_and(|id| {
            !CATALOG
                .iter()
                .any(|entry| entry.task == "transcription" && entry.id == id)
        })
    {
        settings.active_model_transcription = None;
    }
    settings
}

/// Update one known setting and persist the full store; rejects an unknown
/// key or an invalid value without touching the file.
pub fn set_setting(app_support_dir: &Path, key: &str, value: &str) -> Result<Settings> {
    let _write_guard = settings_write_gate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
            crate::asr::resolve_selection(
                value,
                crate::asr::AsrMode::Meeting,
                crate::asr::AsrLanguage::Auto,
            )?;
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
            } else if !CATALOG
                .iter()
                .any(|entry| entry.task == "llm" && entry.id == value)
            {
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
        KEY_CLOUD_PROVIDER => {
            if !matches!(value, "deepgram" | "assemblyai" | "openai") {
                return Err(AppError::InvalidSetting(format!(
                    "unknown cloud provider id: {value}"
                )));
            }
            settings.cloud_provider = value.to_string();
        }
        KEY_RECORDER_SHORTCUT => {
            settings.recorder_shortcut = crate::recorder_shortcut::RecorderShortcut::parse(value)
                .map_err(AppError::InvalidSetting)?
                .as_str()
                .to_string();
        }
        KEY_BUBBLE_ALWAYS_ON_TOP => {
            settings.bubble_always_on_top = parse_bool_setting(KEY_BUBBLE_ALWAYS_ON_TOP, value)?;
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

pub fn set_bubble_position(app_support_dir: &Path, x: f64, y: f64) -> Result<Settings> {
    if !x.is_finite() || !y.is_finite() {
        return Err(AppError::InvalidSetting(
            "bubble position must contain finite coordinates".into(),
        ));
    }
    let _write_guard = settings_write_gate()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut settings = get_settings(app_support_dir);
    settings.bubble_x = Some(x);
    settings.bubble_y = Some(y);
    write_settings(app_support_dir, &settings)?;
    Ok(settings)
}

fn write_settings(app_support_dir: &Path, settings: &Settings) -> Result<()> {
    std::fs::create_dir_all(app_support_dir)?;
    let json = serde_json::to_string_pretty(settings).map_err(|e| AppError::Io(e.to_string()))?;
    let target = settings_path(app_support_dir);
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = app_support_dir.join(format!(
        ".{FILE_NAME}.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary, target)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result.map_err(AppError::from)
}

#[cfg(test)]
#[path = "settings/tests.rs"]
mod tests;
