//! Engine- and capability-aware ASR selection. This is the single source of
//! truth for which engine may serve each application mode; callers never
//! infer capabilities from catalog order or asset shape.

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_ASR_MODEL_ID: &str = "transcription";
pub const QWEN3_ASR_17_GGUF_MODEL_ID: &str = "qwen3-asr-1.7b-q8_0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsrEngine {
    Whisper,
    Qwen3Asr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrRuntime {
    WhisperCpp,
    LlamaCppMtmd,
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
                "ASR language must be auto, ru, or en, got {other}"
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
    pub runtime: AsrRuntime,
    pub capabilities: AsrCapabilities,
    pub compatible_modes: &'static [AsrMode],
    pub min_memory_gb: u16,
    pub license: &'static str,
    /// Stable identity of the exact verified asset bundle. It is part of the
    /// model cache key, so changing any shipped asset revision invalidates it.
    pub asset_fingerprint: &'static str,
}

const ALL_MODES: &[AsrMode] = &[AsrMode::Meeting, AsrMode::Streaming, AsrMode::Recorder];

pub const ASR_SPECS: &[AsrModelSpec] = &[
    AsrModelSpec {
        model_id: DEFAULT_ASR_MODEL_ID,
        engine: AsrEngine::Whisper,
        runtime: AsrRuntime::WhisperCpp,
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
        model_id: QWEN3_ASR_17_GGUF_MODEL_ID,
        engine: AsrEngine::Qwen3Asr,
        runtime: AsrRuntime::LlamaCppMtmd,
        capabilities: AsrCapabilities {
            offline: true,
            streaming: true,
            timestamps: false,
            language_detection: true,
            mixed_language: true,
        },
        compatible_modes: ALL_MODES,
        min_memory_gb: 8,
        license: "Apache-2.0",
        asset_fingerprint: concat!(
            "model:58e22d0532d4eacaf034cfac17a6fed159f37c41390c710186783be439d1fc57;",
            "mmproj:46c1d533af3f354ceb37ce855dbceff7da7fa7cf1e6a523df3b13440bd164c0d"
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
    let _ = language;
    Ok(spec)
}
