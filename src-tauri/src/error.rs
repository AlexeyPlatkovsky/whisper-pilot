//! Application error type, serialized to the frontend as a plain string.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("ffmpeg is required but was not found on PATH")]
    FfmpegMissing,

    #[error("ffmpeg failed to extract audio: {0}")]
    Ffmpeg(String),

    #[error("could not read audio: {0}")]
    Audio(String),

    #[error("transcription model not found at {0} — set MFUPILOT_MODEL_PATH")]
    ModelNotFound(String),

    #[error("failed to load transcription model: {0}")]
    ModelLoad(String),

    #[error("transcription failed: {0}")]
    Transcribe(String),

    #[error("{0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, AppError>;

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

// Tauri commands return errors to JS; expose the human-readable message.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
