//! Engine- and capability-aware ASR selection. This is the single source of
//! truth for which engine may serve each application mode; callers never
//! infer capabilities from catalog order or asset shape.

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_ASR_MODEL_ID: &str = "transcription";
pub const QWEN3_ASR_06_MODEL_ID: &str = "qwen3-asr-0.6b";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsrEngine {
    Whisper,
    Qwen3Asr,
}

impl AsrEngine {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Whisper => "whisper",
            Self::Qwen3Asr => "qwen3_asr",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsrMode {
    Meeting,
    Streaming,
    Recorder,
}

impl AsrMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Meeting => "Meeting",
            Self::Streaming => "Streaming",
            Self::Recorder => "Recorder",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AsrLanguage {
    Auto,
    Russian,
    English,
}

impl AsrLanguage {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "ru" => Ok(Self::Russian),
            "en" => Ok(Self::English),
            other => Err(AppError::InvalidSetting(format!(
                "recorder_language must be auto, ru, or en, got {other}"
            ))),
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Russian => "ru",
            Self::English => "en",
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) const fn qwen_name(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Russian => Some("Russian"),
            Self::English => Some("English"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AsrCapabilities {
    pub offline: bool,
    pub streaming: bool,
    pub timestamps: bool,
    pub language_detection: bool,
    pub mixed_language: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AsrModelSpec {
    pub model_id: &'static str,
    pub engine: AsrEngine,
    pub capabilities: AsrCapabilities,
    pub compatible_modes: &'static [AsrMode],
    pub min_memory_gb: u16,
    pub license: &'static str,
    /// Stable identity of the exact verified asset bundle. It is part of the
    /// model cache key, so changing any shipped asset revision invalidates it.
    pub asset_fingerprint: &'static str,
}

const ALL_MODES: &[AsrMode] = &[AsrMode::Meeting, AsrMode::Streaming, AsrMode::Recorder];
const RECORDER_ONLY: &[AsrMode] = &[AsrMode::Recorder];

pub const ASR_SPECS: &[AsrModelSpec] = &[
    AsrModelSpec {
        model_id: DEFAULT_ASR_MODEL_ID,
        engine: AsrEngine::Whisper,
        capabilities: AsrCapabilities {
            offline: true,
            streaming: true,
            timestamps: true,
            language_detection: true,
            mixed_language: true,
        },
        compatible_modes: ALL_MODES,
        min_memory_gb: 8,
        license: "MIT",
        asset_fingerprint: "317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1",
    },
    AsrModelSpec {
        model_id: QWEN3_ASR_06_MODEL_ID,
        engine: AsrEngine::Qwen3Asr,
        capabilities: AsrCapabilities {
            offline: true,
            streaming: false,
            timestamps: false,
            language_detection: false,
            mixed_language: false,
        },
        compatible_modes: RECORDER_ONLY,
        min_memory_gb: 12,
        license: "Apache-2.0 (model), MIT (runtime)",
        asset_fingerprint: concat!(
            "weights:79d6cbd4c98c7bbffe9db2edac07f56cd6637d0d5944b27f6c2b8353840323ea;",
            "vocab:ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910;",
            "merges:8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5"
        ),
    },
];

pub fn spec_by_id(model_id: &str) -> Option<&'static AsrModelSpec> {
    ASR_SPECS.iter().find(|spec| spec.model_id == model_id)
}

pub fn resolve_selection(
    model_id: &str,
    mode: AsrMode,
    language: AsrLanguage,
) -> Result<&'static AsrModelSpec> {
    let spec = spec_by_id(model_id)
        .ok_or_else(|| AppError::InvalidSetting(format!("unknown ASR model id: {model_id}")))?;
    if !spec.compatible_modes.contains(&mode) {
        return Err(AppError::InvalidSetting(format!(
            "{} is not available for {}; select Whisper instead",
            model_id,
            mode.as_str()
        )));
    }
    if spec.engine == AsrEngine::Qwen3Asr && language == AsrLanguage::Auto {
        return Err(AppError::InvalidSetting(
            "Qwen3-ASR requires Recorder language Russian or English; use Whisper for Auto or mixed speech"
                .into(),
        ));
    }
    Ok(spec)
}
