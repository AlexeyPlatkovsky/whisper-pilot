//! Renderer DTOs and store-to-IPC projection for Recorder commands.

use crate::error::{AppError, Result};
use crate::recorder_store::{
    RecorderSegment, RecorderSession, RecorderSessionId, RecorderStatus, RecorderStore,
};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecorderSessionDto {
    pub(crate) id: i64,
    pub(crate) title: String,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) duration_ms: i64,
    pub(crate) status: RecorderStatus,
    pub(crate) is_draft: bool,
    pub(crate) sample_rate: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recovery_reason: Option<String>,
    pub(crate) audio_path: String,
    pub(crate) asr_model_id: String,
    pub(crate) asr_engine: String,
    pub(crate) asr_language: String,
    pub(crate) segments: Vec<RecorderSegment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) polished_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecorderSessionSummaryDto {
    pub(crate) id: i64,
    pub(crate) title: String,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) duration_ms: i64,
    pub(crate) status: RecorderStatus,
    pub(crate) is_draft: bool,
    pub(crate) sample_rate: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recovery_reason: Option<String>,
    pub(crate) audio_path: String,
    pub(crate) asr_model_id: String,
    pub(crate) asr_engine: String,
    pub(crate) asr_language: String,
}

impl From<RecorderSession> for RecorderSessionSummaryDto {
    fn from(session: RecorderSession) -> Self {
        Self {
            id: session.id,
            title: session.title,
            created_at_ms: session.created_at_ms,
            updated_at_ms: session.updated_at_ms,
            duration_ms: session.duration_ms,
            status: session.status,
            is_draft: session.is_draft,
            sample_rate: session.sample_rate,
            recovery_reason: session.recovery_reason,
            audio_path: session.audio_path.to_string_lossy().into_owned(),
            asr_model_id: session.asr_model_id,
            asr_engine: session.asr_engine,
            asr_language: session.asr_language,
        }
    }
}

fn session_dto(store: &RecorderStore, session: RecorderSession) -> Result<RecorderSessionDto> {
    Ok(RecorderSessionDto {
        id: session.id,
        title: session.title,
        created_at_ms: session.created_at_ms,
        updated_at_ms: session.updated_at_ms,
        duration_ms: session.duration_ms,
        status: session.status,
        is_draft: session.is_draft,
        sample_rate: session.sample_rate,
        recovery_reason: session.recovery_reason,
        audio_path: session.audio_path.to_string_lossy().into_owned(),
        asr_model_id: session.asr_model_id,
        asr_engine: session.asr_engine,
        asr_language: session.asr_language,
        segments: store.list_segments(session.id)?,
        polished_text: store.get_polished(session.id)?,
    })
}

pub(crate) fn open_dto(
    app_support_dir: &Path,
    id: RecorderSessionId,
) -> Result<RecorderSessionDto> {
    let store = RecorderStore::open_runtime(app_support_dir)?;
    let session = store
        .get_session(id)?
        .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))?;
    session_dto(&store, session)
}
