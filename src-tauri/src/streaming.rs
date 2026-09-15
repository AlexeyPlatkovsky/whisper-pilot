//! Testable persistence facade behind the Streaming Tauri commands —
//! parallel to `meetings.rs`, matching its "open the store fresh per call"
//! convention rather than caching a connection in `AppState`.

use crate::cloud_provider::CloudProvider;
use crate::error::{AppError, Result};
use crate::streaming_store::{self, NewStreamingSession, StreamingSessionId, StreamingStore};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingSessionSummaryDto {
    pub id: StreamingSessionId,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub duration_ms: i64,
    pub status: String,
    pub translation_enabled: bool,
    pub translation_target_language: String,
}

/// Immutable engine provenance for one Streaming session. The UI supplies a
/// choice only before the first start; subsequent resumes must use this
/// stored configuration, never a newly selected provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingStartConfiguration {
    Local,
    Cloud(CloudProvider),
}

/// Durable cursor required to resume both persistence indexes and the audio
/// timeline without resetting timestamps to zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamingResumeMetadata {
    pub next_window_index: u64,
    pub last_persisted_end_ms: u64,
}

/// Maps a provider connection's relative final timestamp onto the persisted
/// session timeline. Every non-saturated final advances at least one
/// millisecond so repeated or zero provider timestamps cannot overlap the
/// prior committed window.
pub fn resumed_cloud_window_bounds(
    timeline_offset_ms: i64,
    previous_end_ms: i64,
    connection_end_ms: i64,
) -> (i64, i64) {
    let end_ms = timeline_offset_ms
        .saturating_add(connection_end_ms.max(0))
        .max(previous_end_ms.saturating_add(1));
    (previous_end_ms, end_ms)
}

impl StreamingStartConfiguration {
    fn into_store(self) -> streaming_store::StreamingSessionConfiguration {
        match self {
            Self::Local => streaming_store::StreamingSessionConfiguration {
                engine: "local".to_string(),
                cloud_provider: None,
                cloud_model: None,
            },
            Self::Cloud(provider) => streaming_store::StreamingSessionConfiguration {
                engine: "cloud".to_string(),
                cloud_provider: Some(provider.id().to_string()),
                cloud_model: Some(provider.transport_model().to_string()),
            },
        }
    }

    fn from_store(configuration: streaming_store::StreamingSessionConfiguration) -> Result<Self> {
        match (
            configuration.engine.as_str(),
            configuration.cloud_provider.as_deref(),
            configuration.cloud_model.as_deref(),
        ) {
            ("local", None, None) => Ok(Self::Local),
            ("cloud", Some(provider), Some(model)) => {
                let provider = CloudProvider::try_from(provider)?;
                if model != provider.transport_model() {
                    return Err(AppError::Store(
                        "Meeting has an unknown Cloud model configuration".to_string(),
                    ));
                }
                Ok(Self::Cloud(provider))
            }
            _ => Err(AppError::Store(
                "Meeting has an invalid engine configuration".to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingWindowDto {
    pub window_index: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub language: String,
    pub outcome_ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingMfuDto {
    pub summary: String,
    pub decisions: String,
    pub action_items: String,
    pub open_questions: String,
    pub participants: String,
}

/// One persisted window translation (WP-93; one row per window rather than
/// per paragraph as of WP-103) — the read counterpart to
/// `translate_streaming_window`'s single string return, letting the
/// frontend reuse an already-translated window instead of re-running the
/// model. `source_text` rides along so a caller holding the *current*
/// window's text can detect a stale row — the window's text changed since
/// it was translated (e.g. a fail-open retry) — without this needing any
/// window-grouping concept of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingTranslationDto {
    pub window_index: i64,
    pub source_text: String,
    pub translated_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingSessionDto {
    pub id: StreamingSessionId,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub status: String,
    pub windows: Vec<StreamingWindowDto>,
    pub mfu: Option<StreamingMfuDto>,
    pub prettified_text: Option<String>,
    pub translation_enabled: bool,
    pub translation_target_language: String,
    /// The persisted, non-secret transcription engine. This lets the UI avoid
    /// presenting a stopped Cloud session as a Local resume target.
    pub transcription_engine: Option<String>,
}

pub fn list_streaming_sessions(app_support_dir: &Path) -> Result<Vec<StreamingSessionSummaryDto>> {
    let summaries = StreamingStore::open(app_support_dir)?
        .list_sessions()?
        .into_iter()
        .map(|s| StreamingSessionSummaryDto {
            id: s.id,
            title: s.title,
            created_at_ms: s.created_at_ms,
            updated_at_ms: s.updated_at_ms,
            duration_ms: s.duration_ms,
            status: s.status,
            translation_enabled: s.translation_enabled,
            translation_target_language: s.translation_target_language,
        })
        .collect();
    Ok(summaries)
}

pub fn open_streaming_session(
    app_support_dir: &Path,
    id: StreamingSessionId,
) -> Result<StreamingSessionDto> {
    let store = StreamingStore::open(app_support_dir)?;
    let session = store
        .get_session(id)?
        .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;
    let windows = store
        .list_windows(id)?
        .into_iter()
        .map(|w| StreamingWindowDto {
            window_index: w.window_index,
            start_ms: w.start_ms,
            end_ms: w.end_ms,
            text: w.text,
            language: w.language,
            outcome_ok: w.outcome_ok,
        })
        .collect();
    let mfu = store.get_mfu(id)?.map(|n| StreamingMfuDto {
        summary: n.summary,
        decisions: n.decisions,
        action_items: n.action_items,
        open_questions: n.open_questions,
        participants: n.participants,
    });
    let prettified_text = store.get_prettified(id)?;
    let transcription_engine = store
        .get_session_configuration(id)?
        .map(StreamingStartConfiguration::from_store)
        .transpose()?
        .map(|configuration| match configuration {
            StreamingStartConfiguration::Local => "local".to_string(),
            StreamingStartConfiguration::Cloud(_) => "cloud".to_string(),
        });
    Ok(StreamingSessionDto {
        id: session.id,
        title: session.title,
        created_at_ms: session.created_at_ms,
        updated_at_ms: session.updated_at_ms,
        status: session.status,
        windows,
        mfu,
        prettified_text,
        translation_enabled: session.translation_enabled,
        translation_target_language: session.translation_target_language,
        transcription_engine,
    })
}

/// Enforced here, not just via the frontend's disabled button, since a
/// direct call bypasses that guard entirely. Fail-open windows are excluded
/// — `[unavailable]` is a UI-display artifact, not real content.
pub fn build_streaming_transcript(
    app_support_dir: &Path,
    id: StreamingSessionId,
) -> Result<String> {
    let store = StreamingStore::open(app_support_dir)?;
    let session = store
        .get_session(id)?
        .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;
    if session.status != streaming_store::status::STOPPED {
        return Err(AppError::Llm(
            "cannot craft MFU while a Meeting is still active".into(),
        ));
    }
    let transcript = store
        .list_windows(id)?
        .into_iter()
        .filter(|w| w.outcome_ok)
        .map(|w| w.text)
        .collect::<Vec<_>>()
        .join(" ");
    if transcript.trim().is_empty() {
        return Err(AppError::Llm(
            "meeting has no transcript to summarize".into(),
        ));
    }
    Ok(transcript)
}

pub fn rename_streaming_session(
    app_support_dir: &Path,
    id: StreamingSessionId,
    title: String,
) -> Result<StreamingSessionDto> {
    StreamingStore::open(app_support_dir)?.rename_session(id, &title)?;
    open_streaming_session(app_support_dir, id)
}

pub fn delete_streaming_session(app_support_dir: &Path, id: StreamingSessionId) -> Result<()> {
    StreamingStore::open(app_support_dir)?.delete_session(id)
}

pub fn clear_streaming_session(
    app_support_dir: &Path,
    id: StreamingSessionId,
) -> Result<StreamingSessionDto> {
    StreamingStore::open(app_support_dir)?.clear_session_content(id)?;
    open_streaming_session(app_support_dir, id)
}

/// Create a new, stopped session record without beginning audio capture.
/// Titled by creation time (matching Meeting's plain default title) — the
/// user can rename it, then explicitly start it when ready.
pub fn create_streaming_session(
    app_support_dir: &Path,
    created_at_ms: i64,
) -> Result<StreamingSessionId> {
    let store = StreamingStore::open(app_support_dir)?;
    let session = store.create_session(NewStreamingSession {
        title: "New Meeting".to_string(),
        created_at_ms,
    })?;
    Ok(session.id)
}

/// Prepare a previously-stopped session to resume capturing: validates it is
/// actually stopped (an active or nonexistent session cannot be resumed),
/// marks it active again, and returns the window index the decode loop
/// should continue counting from — one past the last persisted window, or 0
/// for a session that was stopped before any window was ever saved.
pub fn resume_streaming_session(
    app_support_dir: &Path,
    id: StreamingSessionId,
    now_ms: i64,
) -> Result<(StreamingSessionSummaryDto, StreamingResumeMetadata)> {
    let store = StreamingStore::open(app_support_dir)?;
    let session = store
        .get_session(id)?
        .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;
    if session.status != streaming_store::status::STOPPED {
        return Err(AppError::Capture(
            "only a stopped Meeting can be resumed".into(),
        ));
    }
    let windows = store.list_windows(id)?;
    let resume = windows
        .last()
        .map(|window| StreamingResumeMetadata {
            next_window_index: u64::try_from(window.window_index)
                .map_or(0, |index| index.saturating_add(1)),
            last_persisted_end_ms: window.end_ms.max(0) as u64,
        })
        .unwrap_or(StreamingResumeMetadata {
            next_window_index: 0,
            last_persisted_end_ms: 0,
        });
    let duration_ms = resume.last_persisted_end_ms.min(i64::MAX as u64) as i64;
    store.mark_active(id, now_ms)?;
    Ok((
        StreamingSessionSummaryDto {
            id: session.id,
            title: session.title,
            created_at_ms: session.created_at_ms,
            updated_at_ms: now_ms,
            duration_ms,
            status: streaming_store::status::ACTIVE.to_string(),
            translation_enabled: session.translation_enabled,
            translation_target_language: session.translation_target_language,
        },
        resume,
    ))
}

/// Persists a session's engine choice before the first capture and returns
/// the durable configuration used by the caller to construct Local or Cloud
/// runtime work. Resuming always reuses that stored configuration, regardless
/// of the current settings choice.
pub fn prepare_streaming_session_start(
    app_support_dir: &Path,
    id: StreamingSessionId,
    requested: Option<StreamingStartConfiguration>,
    now_ms: i64,
) -> Result<(
    StreamingSessionSummaryDto,
    StreamingResumeMetadata,
    StreamingStartConfiguration,
)> {
    let store = StreamingStore::open(app_support_dir)?;
    let configured = match store.get_session_configuration(id)? {
        Some(configuration) => StreamingStartConfiguration::from_store(configuration)?,
        None => {
            let requested = requested.unwrap_or(StreamingStartConfiguration::Local);
            store.set_session_configuration(id, &requested.into_store())?;
            requested
        }
    };
    let (summary, resume) = resume_streaming_session(app_support_dir, id, now_ms)?;
    Ok((summary, resume, configured))
}

/// Persists the Live Translation on/off choice for one session (WP-101) —
/// the facade counterpart to `rename_streaming_session`, delegating straight
/// to the store's single-field update.
pub fn set_streaming_translation_enabled(
    app_support_dir: &Path,
    id: StreamingSessionId,
    enabled: bool,
) -> Result<()> {
    StreamingStore::open(app_support_dir)?.set_translation_enabled(id, enabled)
}

pub fn set_streaming_translation_target_language(
    app_support_dir: &Path,
    id: StreamingSessionId,
    target_language: &str,
) -> Result<()> {
    StreamingStore::open(app_support_dir)?.set_translation_target_language(id, target_language)
}

/// Validates a translation request's cheap, model-independent prerequisites
/// — target language supported, session exists, source text non-empty — so
/// a doomed request never reaches the LLM. Run before resolving the model
/// path or acquiring the translation single-flight guard.
pub fn ensure_translation_request_is_valid(
    app_support_dir: &Path,
    session_id: StreamingSessionId,
    target_language: &str,
    text: &str,
) -> Result<()> {
    if !crate::llm::is_supported_target_language(target_language) {
        return Err(AppError::Llm(format!(
            "unsupported translation target language: {target_language}"
        )));
    }
    let store = StreamingStore::open(app_support_dir)?;
    store
        .get_session(session_id)?
        .ok_or_else(|| AppError::Store(format!("meeting {session_id} was not found")))?;
    if text.trim().is_empty() {
        return Err(AppError::Llm(
            "cannot translate an empty or whitespace-only paragraph".into(),
        ));
    }
    Ok(())
}

/// Runs `translate` (production: `llm::translate_paragraph` bound to the
/// resolved model path; tests: a fake) and persists the result — the part
/// of `translate_streaming_window` that needs the shared LLM. Injected so
/// this (including its "write no row on failure" guarantee) is exercisable
/// without a real model or Tauri `AppHandle`. `context` (WP-100) passes
/// through to `translate` unchanged as ephemeral prompt context — never
/// itself persisted.
#[allow(clippy::too_many_arguments)]
pub fn translate_and_store(
    app_support_dir: &Path,
    session_id: StreamingSessionId,
    window_index: i64,
    target_language: &str,
    text: &str,
    context: Option<&str>,
    now_ms: i64,
    translate: impl FnOnce(&str, &str, Option<&str>) -> Result<String>,
) -> Result<String> {
    translate_and_store_after_check(
        app_support_dir,
        session_id,
        window_index,
        target_language,
        text,
        context,
        now_ms,
        translate,
        || Ok(()),
    )
}

/// Production translation path: after inference, re-check the persisted
/// toggle and source window before writing. This makes a UI cancellation or
/// a revised source authoritative even when inference was already running.
#[allow(clippy::too_many_arguments)]
pub fn translate_and_store_if_current(
    app_support_dir: &Path,
    session_id: StreamingSessionId,
    window_index: i64,
    target_language: &str,
    text: &str,
    context: Option<&str>,
    now_ms: i64,
    translate: impl FnOnce(&str, &str, Option<&str>) -> Result<String>,
) -> Result<String> {
    let store = StreamingStore::open(app_support_dir)?;
    if !store.translation_source_is_available(session_id, window_index, text)? {
        return Err(AppError::Llm(
            "translation source is unavailable or changed before inference".to_string(),
        ));
    }
    let translated = translate(text, target_language, context)?;
    let saved = store.upsert_translation_if_current(&streaming_store::StreamingTranslation {
        session_id,
        window_index,
        target_language: target_language.to_string(),
        source_text: text.to_string(),
        translated_text: translated.clone(),
        updated_at_ms: now_ms,
    })?;
    if !saved {
        return Err(AppError::Llm(
            "translation was cancelled or its source changed before it could be saved".to_string(),
        ));
    }
    Ok(translated)
}

#[allow(clippy::too_many_arguments)]
fn translate_and_store_after_check(
    app_support_dir: &Path,
    session_id: StreamingSessionId,
    window_index: i64,
    target_language: &str,
    text: &str,
    context: Option<&str>,
    now_ms: i64,
    translate: impl FnOnce(&str, &str, Option<&str>) -> Result<String>,
    post_inference_check: impl FnOnce() -> Result<()>,
) -> Result<String> {
    let translated = translate(text, target_language, context)?;
    post_inference_check()?;

    let store = StreamingStore::open(app_support_dir)?;
    store.upsert_translation(&streaming_store::StreamingTranslation {
        session_id,
        window_index,
        target_language: target_language.to_string(),
        source_text: text.to_string(),
        translated_text: translated.clone(),
        updated_at_ms: now_ms,
    })?;

    Ok(translated)
}

/// All persisted translations for one session and target language (WP-93) —
/// the read counterpart to `translate_streaming_window`. Replaying a whole
/// session's windows through the single-flight model on every "Live
/// Translation On" would be slow and wasteful for a session with translation
/// history, so the frontend loads this first and only calls the model for
/// windows missing here or whose `source_text` no longer matches.
pub fn list_streaming_translations(
    app_support_dir: &Path,
    session_id: StreamingSessionId,
    target_language: &str,
) -> Result<Vec<StreamingTranslationDto>> {
    if !crate::llm::is_supported_target_language(target_language) {
        return Err(AppError::Llm(format!(
            "unsupported translation target language: {target_language}"
        )));
    }
    let store = StreamingStore::open(app_support_dir)?;
    store
        .get_session(session_id)?
        .ok_or_else(|| AppError::Store(format!("meeting {session_id} was not found")))?;
    let translations = store
        .list_translations(session_id, target_language)?
        .into_iter()
        .map(|t| StreamingTranslationDto {
            window_index: t.window_index,
            source_text: t.source_text,
            translated_text: t.translated_text,
        })
        .collect();
    Ok(translations)
}

#[cfg(test)]
mod tests;
