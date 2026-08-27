//! Testable persistence facade behind the Streaming Tauri commands —
//! parallel to `meetings.rs`, matching its "open the store fresh per call"
//! convention rather than caching a connection in `AppState`.

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
    pub status: String,
    pub translation_enabled: bool,
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
            status: s.status,
            translation_enabled: s.translation_enabled,
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
        .ok_or_else(|| AppError::Store(format!("streaming session {id} was not found")))?;
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
        .ok_or_else(|| AppError::Store(format!("streaming session {id} was not found")))?;
    if session.status != streaming_store::status::STOPPED {
        return Err(AppError::Llm(
            "cannot craft mfu while a Streaming session is still active".into(),
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
            "streaming session has no transcript to summarize".into(),
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

/// Create a new, stopped session record without beginning audio capture.
/// Titled by creation time (matching Meeting's plain default title) — the
/// user can rename it, then explicitly start it when ready.
pub fn create_streaming_session(
    app_support_dir: &Path,
    created_at_ms: i64,
) -> Result<StreamingSessionId> {
    let store = StreamingStore::open(app_support_dir)?;
    let session = store.create_session(NewStreamingSession {
        title: "New Streaming Session".to_string(),
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
) -> Result<(StreamingSessionSummaryDto, u64)> {
    let store = StreamingStore::open(app_support_dir)?;
    let session = store
        .get_session(id)?
        .ok_or_else(|| AppError::Store(format!("streaming session {id} was not found")))?;
    if session.status != streaming_store::status::STOPPED {
        return Err(AppError::Capture(
            "only a stopped Streaming session can be resumed".into(),
        ));
    }
    let next_window_index = store
        .list_windows(id)?
        .last()
        .map(|w| w.window_index as u64 + 1)
        .unwrap_or(0);
    store.mark_active(id, now_ms)?;
    Ok((
        StreamingSessionSummaryDto {
            id: session.id,
            title: session.title,
            created_at_ms: session.created_at_ms,
            updated_at_ms: now_ms,
            status: streaming_store::status::ACTIVE.to_string(),
            translation_enabled: session.translation_enabled,
        },
        next_window_index,
    ))
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
        .ok_or_else(|| AppError::Store(format!("streaming session {session_id} was not found")))?;
    if text.trim().is_empty() {
        return Err(AppError::Llm(
            "cannot translate an empty or whitespace-only paragraph".into(),
        ));
    }
    Ok(())
}

/// Runs `translate` (production: `llm::translate_paragraph` bound to the
/// resolved model path; tests: a fake, so this composition is verifiable
/// without a real model) and persists the result — the part of
/// `translate_streaming_window` that needs the shared LLM. Injected so
/// this can be exercised, including its "write no row on failure"
/// guarantee, without a real model or Tauri `AppHandle`. `context` (WP-100)
/// is the immediately preceding window(s)' own translation, passed through
/// to `translate` unchanged as ephemeral prompt context — it is never
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
    let translated = translate(text, target_language, context)?;

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
        .ok_or_else(|| AppError::Store(format!("streaming session {session_id} was not found")))?;
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
mod tests {
    use super::*;
    use crate::streaming_store::NewStreamingWindow;

    #[test]
    fn given_no_sessions_when_listing_then_result_is_empty() {
        let temp = tempfile::tempdir().expect("temp dir");
        assert!(list_streaming_sessions(temp.path())
            .expect("list")
            .is_empty());
    }

    #[test]
    fn create_then_open_round_trips_with_no_windows() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let dto = open_streaming_session(temp.path(), id).expect("open");

        assert_eq!(dto.id, id);
        assert_eq!(dto.title, "New Streaming Session");
        assert_eq!(dto.status, streaming_store::status::STOPPED);
        assert!(dto.windows.is_empty());
    }

    #[test]
    fn opening_an_unknown_session_is_a_store_error() {
        let temp = tempfile::tempdir().expect("temp dir");
        assert!(open_streaming_session(temp.path(), 999_999).is_err());
    }

    #[test]
    fn rename_then_open_reflects_the_new_title() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let dto =
            rename_streaming_session(temp.path(), id, "Team standup".to_string()).expect("rename");

        assert_eq!(dto.title, "Team standup");
    }

    #[test]
    fn delete_then_open_reports_not_found() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        delete_streaming_session(temp.path(), id).expect("delete");

        assert!(open_streaming_session(temp.path(), id).is_err());
    }

    // S-1: happy path — a stopped session with saved windows resumes from
    // one past the last window index, and its status/freshness update.
    #[test]
    fn resuming_a_stopped_session_with_windows_continues_from_the_next_index() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = StreamingStore::open(temp.path()).expect("open store");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        store
            .append_window(
                id,
                &NewStreamingWindow {
                    window_index: 0,
                    start_ms: 0,
                    end_ms: 7_000,
                    text: "hello".to_string(),
                    language: "en".to_string(),
                    outcome_ok: true,
                },
                7_100,
            )
            .expect("append window 0");
        store
            .append_window(
                id,
                &NewStreamingWindow {
                    window_index: 1,
                    start_ms: 7_000,
                    end_ms: 14_000,
                    text: "there".to_string(),
                    language: "en".to_string(),
                    outcome_ok: true,
                },
                14_100,
            )
            .expect("append window 1");
        store.mark_stopped(id, 15_000).expect("mark stopped");

        let (summary, next_window_index) =
            resume_streaming_session(temp.path(), id, 20_000).expect("resume");

        assert_eq!(next_window_index, 2);
        assert_eq!(summary.status, streaming_store::status::ACTIVE);
        assert_eq!(summary.updated_at_ms, 20_000);
        assert_eq!(summary.title, "New Streaming Session");
        let reloaded = store
            .get_session(id)
            .expect("get session")
            .expect("session exists");
        assert_eq!(reloaded.status, streaming_store::status::ACTIVE);
    }

    // BVA: a stopped session that never saved a window resumes at index 0,
    // the lower boundary — not an out-of-range or panicking `.last()`.
    #[test]
    fn resuming_a_stopped_session_with_no_windows_starts_at_index_zero() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = StreamingStore::open(temp.path()).expect("open store");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        store.mark_stopped(id, 500).expect("mark stopped");

        let (_summary, next_window_index) =
            resume_streaming_session(temp.path(), id, 600).expect("resume");

        assert_eq!(next_window_index, 0);
    }

    // S-2, decision-table: status=active is the one rejected cell — resuming
    // an already-running session would double-capture into it.
    #[test]
    fn resuming_an_active_session_is_rejected() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        StreamingStore::open(temp.path())
            .expect("open store")
            .mark_active(id, 150)
            .expect("mark active");

        assert!(resume_streaming_session(temp.path(), id, 200).is_err());
    }

    #[test]
    fn resuming_a_nonexistent_session_is_a_store_error() {
        let temp = tempfile::tempdir().expect("temp dir");

        assert!(resume_streaming_session(temp.path(), 999_999, 100).is_err());
    }

    #[test]
    fn open_returns_windows_in_order_with_all_fields() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        let store = StreamingStore::open(temp.path()).expect("open store");
        store
            .append_window(
                id,
                &NewStreamingWindow {
                    window_index: 0,
                    start_ms: 0,
                    end_ms: 7_000,
                    text: "hello".to_string(),
                    language: "en".to_string(),
                    outcome_ok: true,
                },
                7_100,
            )
            .expect("append window");

        let dto = open_streaming_session(temp.path(), id).expect("open");

        assert_eq!(
            dto.windows,
            vec![StreamingWindowDto {
                window_index: 0,
                start_ms: 0,
                end_ms: 7_000,
                text: "hello".to_string(),
                language: "en".to_string(),
                outcome_ok: true,
            }]
        );
    }

    fn stopped_session_with_windows(
        temp: &std::path::Path,
        windows: &[NewStreamingWindow],
    ) -> StreamingSessionId {
        let store = StreamingStore::open(temp).expect("open store");
        let id = create_streaming_session(temp, 100).expect("create");
        for (i, w) in windows.iter().enumerate() {
            store
                .append_window(id, w, 100 + i as i64)
                .expect("append window");
        }
        store.mark_stopped(id, 999).expect("mark stopped");
        id
    }

    fn ok_window(index: i64, text: &str) -> NewStreamingWindow {
        NewStreamingWindow {
            window_index: index,
            start_ms: index * 7_000,
            end_ms: (index + 1) * 7_000,
            text: text.to_string(),
            language: "en".to_string(),
            outcome_ok: true,
        }
    }

    fn failed_window(index: i64) -> NewStreamingWindow {
        NewStreamingWindow {
            window_index: index,
            start_ms: index * 7_000,
            end_ms: (index + 1) * 7_000,
            text: String::new(),
            language: "auto".to_string(),
            outcome_ok: false,
        }
    }

    // S-5, EP: outcome_ok=true vs false windows are two input partitions.
    #[test]
    fn build_transcript_joins_only_ok_windows_in_order() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = stopped_session_with_windows(
            temp.path(),
            &[
                ok_window(1, "second"),
                failed_window(0),
                ok_window(2, "third"),
            ],
        );

        let transcript = build_streaming_transcript(temp.path(), id).expect("transcript");

        assert_eq!(transcript, "second third");
    }

    // S-12, decision-table: status=active is the one rejected cell.
    #[test]
    fn build_transcript_errors_on_active_session() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = StreamingStore::open(temp.path()).expect("open store");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        store.mark_active(id, 150).expect("mark active");
        store
            .append_window(id, &ok_window(0, "hello"), 200)
            .expect("append window");

        assert!(build_streaming_transcript(temp.path(), id).is_err());
    }

    // BVA: zero windows is the lower boundary of "no transcript".
    #[test]
    fn build_transcript_errors_on_no_windows() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = stopped_session_with_windows(temp.path(), &[]);

        assert!(build_streaming_transcript(temp.path(), id).is_err());
    }

    // S-6, BVA: all windows present but all outcome_ok=false — the boundary
    // just past "one ok window" (build_transcript_joins_only_ok_windows).
    #[test]
    fn build_transcript_errors_when_all_windows_failed() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = stopped_session_with_windows(temp.path(), &[failed_window(0), failed_window(1)]);

        assert!(build_streaming_transcript(temp.path(), id).is_err());
    }

    #[test]
    fn build_transcript_errors_on_nonexistent_session() {
        let temp = tempfile::tempdir().expect("temp dir");

        assert!(build_streaming_transcript(temp.path(), 999_999).is_err());
    }

    #[test]
    fn open_streaming_session_notes_is_none_when_absent() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let dto = open_streaming_session(temp.path(), id).expect("open");

        assert_eq!(dto.mfu, None);
    }

    #[test]
    fn open_streaming_session_includes_notes_when_present() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = StreamingStore::open(temp.path()).expect("open store");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        store
            .upsert_mfu(&crate::streaming_store::StreamingMfu {
                session_id: id,
                summary: "Summary.".to_string(),
                decisions: "Decisions.".to_string(),
                action_items: "Actions.".to_string(),
                open_questions: "Questions.".to_string(),
                participants: "Alex".to_string(),
            })
            .expect("upsert mfu");

        let dto = open_streaming_session(temp.path(), id).expect("open");

        assert_eq!(
            dto.mfu,
            Some(StreamingMfuDto {
                summary: "Summary.".to_string(),
                decisions: "Decisions.".to_string(),
                action_items: "Actions.".to_string(),
                open_questions: "Questions.".to_string(),
                participants: "Alex".to_string(),
            })
        );
    }

    #[test]
    fn open_streaming_session_prettified_text_is_none_when_absent() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let dto = open_streaming_session(temp.path(), id).expect("open");

        assert_eq!(dto.prettified_text, None);
    }

    #[test]
    fn open_streaming_session_includes_prettified_text_when_present() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = StreamingStore::open(temp.path()).expect("open store");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        store
            .upsert_prettified(id, "Cleaned transcript.")
            .expect("upsert prettified");

        let dto = open_streaming_session(temp.path(), id).expect("open");

        assert_eq!(dto.prettified_text, Some("Cleaned transcript.".to_string()));
    }

    #[test]
    fn streaming_session_dto_round_trips_through_the_ipc_json_contract() {
        let original = StreamingSessionDto {
            id: 7,
            title: "Contract session".to_string(),
            created_at_ms: 42,
            updated_at_ms: 100,
            status: "stopped".to_string(),
            windows: vec![StreamingWindowDto {
                window_index: 0,
                start_ms: 0,
                end_ms: 7_000,
                text: "Saved window".to_string(),
                language: "en".to_string(),
                outcome_ok: true,
            }],
            mfu: Some(StreamingMfuDto {
                summary: "Summary.".to_string(),
                decisions: "Decisions.".to_string(),
                action_items: "Actions.".to_string(),
                open_questions: "Questions.".to_string(),
                participants: "Alex".to_string(),
            }),
            prettified_text: Some("Cleaned transcript.".to_string()),
            translation_enabled: false,
        };

        let json = serde_json::to_value(&original).expect("serialize streaming session DTO");
        let round_tripped: StreamingSessionDto =
            serde_json::from_value(json).expect("deserialize streaming session DTO");

        assert_eq!(round_tripped, original);
    }

    // --- WP-92: translate_streaming_window's testable core ---

    #[test]
    fn ensure_translation_request_is_valid_rejects_an_unsupported_target_language() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let result = ensure_translation_request_is_valid(temp.path(), id, "fr", "Bonjour");

        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    #[test]
    fn ensure_translation_request_is_valid_rejects_an_unknown_session() {
        let temp = tempfile::tempdir().expect("temp dir");

        let result = ensure_translation_request_is_valid(temp.path(), 999_999, "en", "Привет");

        assert!(matches!(result, Err(AppError::Store(_))));
    }

    #[test]
    fn ensure_translation_request_is_valid_rejects_empty_source_text() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let result = ensure_translation_request_is_valid(temp.path(), id, "en", "   \n  ");

        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    #[test]
    fn ensure_translation_request_is_valid_accepts_a_well_formed_request() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let result = ensure_translation_request_is_valid(temp.path(), id, "en", "Привет, мир.");

        assert!(result.is_ok());
    }

    #[test]
    fn translate_and_store_persists_on_success_and_returns_translated_text() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let translated = translate_and_store(
            temp.path(),
            id,
            0,
            "en",
            "Привет, мир.",
            None,
            1_000,
            |_text, _lang, _ctx| Ok("Hello, world.".to_string()),
        )
        .expect("translate and store");

        assert_eq!(translated, "Hello, world.");
        let store = StreamingStore::open(temp.path()).expect("open store");
        let rows = store
            .list_translations(id, "en")
            .expect("list translations");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_text, "Привет, мир.");
        assert_eq!(rows[0].translated_text, "Hello, world.");
    }

    #[test]
    fn translate_and_store_writes_no_row_when_translation_is_rejected() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let result = translate_and_store(
            temp.path(),
            id,
            0,
            "en",
            "Привет, мир.",
            None,
            1_000,
            |_text, _lang, _ctx| Err(AppError::Llm("candidate rejected".into())),
        );

        assert!(result.is_err());
        let store = StreamingStore::open(temp.path()).expect("open store");
        assert!(store
            .list_translations(id, "en")
            .expect("list translations")
            .is_empty());
    }

    #[test]
    fn translate_and_store_overwrites_rather_than_duplicates_for_the_same_window_index() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        translate_and_store(
            temp.path(),
            id,
            0,
            "en",
            "Привет, мир.",
            None,
            1_000,
            |_, _, _| Ok("Hello, world.".to_string()),
        )
        .expect("first translate");
        translate_and_store(
            temp.path(),
            id,
            0,
            "en",
            "Привет, мир.",
            None,
            2_000,
            |_, _, _| Ok("Hi, world.".to_string()),
        )
        .expect("retranslate");

        let store = StreamingStore::open(temp.path()).expect("open store");
        let rows = store
            .list_translations(id, "en")
            .expect("list translations");
        assert_eq!(rows.len(), 1, "retranslation must overwrite, not duplicate");
        assert_eq!(rows[0].translated_text, "Hi, world.");
    }

    // WP-100: translate_and_store threads its `context` parameter straight
    // through to the injected `translate` closure, unchanged.
    #[test]
    fn translate_and_store_passes_context_through_to_translate() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        let mut received_context: Option<String> = None;

        translate_and_store(
            temp.path(),
            id,
            0,
            "en",
            "Привет, мир.",
            Some("Previous paragraph translation."),
            1_000,
            |_text, _lang, ctx| {
                received_context = ctx.map(|c| c.to_string());
                Ok("Hello, world.".to_string())
            },
        )
        .expect("translate and store");

        assert_eq!(
            received_context.as_deref(),
            Some("Previous paragraph translation.")
        );
    }

    // WP-100: absent context (None) must reach the translate closure as
    // None too, unchanged from the pre-WP-100 behavior.
    #[test]
    fn translate_and_store_passes_no_context_when_absent() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        let mut received_context: Option<String> = Some("sentinel".to_string());

        translate_and_store(
            temp.path(),
            id,
            0,
            "en",
            "Привет, мир.",
            None,
            1_000,
            |_text, _lang, ctx| {
                received_context = ctx.map(|c| c.to_string());
                Ok("Hello, world.".to_string())
            },
        )
        .expect("translate and store");

        assert!(received_context.is_none());
    }

    // --- WP-93: list_streaming_translations, the read counterpart to
    // translate_streaming_window the frontend uses to reuse already-
    // persisted translations instead of re-running the model. ---

    #[test]
    fn list_streaming_translations_returns_empty_when_none_are_stored() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let rows = list_streaming_translations(temp.path(), id, "en").expect("list translations");

        assert!(rows.is_empty());
    }

    #[test]
    fn list_streaming_translations_returns_persisted_rows_with_source_text() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        translate_and_store(
            temp.path(),
            id,
            0,
            "en",
            "Привет, мир.",
            None,
            1_000,
            |_, _, _| Ok("Hello, world.".to_string()),
        )
        .expect("translate and store");

        let rows = list_streaming_translations(temp.path(), id, "en").expect("list translations");

        assert_eq!(
            rows,
            vec![StreamingTranslationDto {
                window_index: 0,
                source_text: "Привет, мир.".to_string(),
                translated_text: "Hello, world.".to_string(),
            }]
        );
    }

    #[test]
    fn list_streaming_translations_orders_by_window_index() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        translate_and_store(
            temp.path(),
            id,
            5,
            "en",
            "Second.",
            None,
            1_000,
            |_, _, _| Ok("Second (en).".to_string()),
        )
        .expect("translate and store paragraph 5");
        translate_and_store(
            temp.path(),
            id,
            0,
            "en",
            "First.",
            None,
            1_000,
            |_, _, _| Ok("First (en).".to_string()),
        )
        .expect("translate and store paragraph 0");

        let rows = list_streaming_translations(temp.path(), id, "en").expect("list translations");

        assert_eq!(
            rows.iter().map(|r| r.window_index).collect::<Vec<_>>(),
            vec![0, 5]
        );
    }

    #[test]
    fn list_streaming_translations_scopes_by_target_language() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        translate_and_store(
            temp.path(),
            id,
            0,
            "en",
            "Привет.",
            None,
            1_000,
            |_, _, _| Ok("Hi.".to_string()),
        )
        .expect("translate en");
        translate_and_store(temp.path(), id, 0, "ru", "Hi.", None, 1_000, |_, _, _| {
            Ok("Привет.".to_string())
        })
        .expect("translate ru");

        let en_rows =
            list_streaming_translations(temp.path(), id, "en").expect("list en translations");
        let ru_rows =
            list_streaming_translations(temp.path(), id, "ru").expect("list ru translations");

        assert_eq!(en_rows.len(), 1);
        assert_eq!(en_rows[0].translated_text, "Hi.");
        assert_eq!(ru_rows.len(), 1);
        assert_eq!(ru_rows[0].translated_text, "Привет.");
    }

    #[test]
    fn list_streaming_translations_rejects_an_unsupported_target_language() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let result = list_streaming_translations(temp.path(), id, "fr");

        assert!(matches!(result, Err(AppError::Llm(_))));
    }

    #[test]
    fn list_streaming_translations_rejects_an_unknown_session() {
        let temp = tempfile::tempdir().expect("temp dir");

        let result = list_streaming_translations(temp.path(), 999_999, "en");

        assert!(matches!(result, Err(AppError::Store(_))));
    }

    #[test]
    fn streaming_translation_dto_round_trips() {
        let original = StreamingTranslationDto {
            window_index: 3,
            source_text: "Исходный текст.".to_string(),
            translated_text: "Source text.".to_string(),
        };

        let json = serde_json::to_value(&original).expect("serialize translation DTO");
        let round_tripped: StreamingTranslationDto =
            serde_json::from_value(json).expect("deserialize translation DTO");

        assert_eq!(round_tripped, original);
    }

    // --- WP-103: paragraph_key -> window_index rename, exercised end-to-end
    // through translate_and_store and list_streaming_translations. ---

    #[test]
    fn translate_and_store_and_list_use_window_index_naming_end_to_end() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let translated = translate_and_store(
            temp.path(),
            id,
            7,
            "en",
            "Привет, мир.",
            None,
            1_000,
            |_text, _lang, _ctx| Ok("Hello, world.".to_string()),
        )
        .expect("translate and store window 7");

        assert_eq!(translated, "Hello, world.");
        let rows = list_streaming_translations(temp.path(), id, "en").expect("list translations");
        assert_eq!(
            rows,
            vec![StreamingTranslationDto {
                window_index: 7,
                source_text: "Привет, мир.".to_string(),
                translated_text: "Hello, world.".to_string(),
            }]
        );
    }

    // --- WP-101: translation_enabled on the summary/session DTOs, and the
    // set_streaming_translation_enabled facade function. ---

    #[test]
    fn new_session_summary_and_dto_report_translation_enabled_false() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        let summaries = list_streaming_sessions(temp.path()).expect("list");
        assert_eq!(summaries.len(), 1);
        assert!(!summaries[0].translation_enabled);

        let dto = open_streaming_session(temp.path(), id).expect("open");
        assert!(!dto.translation_enabled);
    }

    #[test]
    fn set_streaming_translation_enabled_is_reflected_by_list_and_open() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");

        set_streaming_translation_enabled(temp.path(), id, true).expect("set translation enabled");

        let summaries = list_streaming_sessions(temp.path()).expect("list");
        assert!(summaries[0].translation_enabled);
        let dto = open_streaming_session(temp.path(), id).expect("open");
        assert!(dto.translation_enabled);
    }

    #[test]
    fn set_streaming_translation_enabled_can_be_turned_back_off() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        set_streaming_translation_enabled(temp.path(), id, true).expect("enable");

        set_streaming_translation_enabled(temp.path(), id, false).expect("disable");

        let dto = open_streaming_session(temp.path(), id).expect("open");
        assert!(!dto.translation_enabled);
    }

    #[test]
    fn set_streaming_translation_enabled_rejects_an_unknown_session() {
        let temp = tempfile::tempdir().expect("temp dir");

        let result = set_streaming_translation_enabled(temp.path(), 999_999, true);

        assert!(matches!(result, Err(AppError::Store(_))));
    }

    #[test]
    fn resume_streaming_session_summary_reflects_persisted_translation_enabled() {
        let temp = tempfile::tempdir().expect("temp dir");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        set_streaming_translation_enabled(temp.path(), id, true).expect("enable");
        StreamingStore::open(temp.path())
            .expect("open store")
            .mark_stopped(id, 500)
            .expect("mark stopped");

        let (summary, _next_index) =
            resume_streaming_session(temp.path(), id, 600).expect("resume");

        assert!(summary.translation_enabled);
    }

    #[test]
    fn streaming_session_dto_round_trips_with_translation_enabled() {
        let original = StreamingSessionDto {
            id: 7,
            title: "Contract session".to_string(),
            created_at_ms: 42,
            updated_at_ms: 100,
            status: "stopped".to_string(),
            windows: vec![],
            mfu: None,
            prettified_text: None,
            translation_enabled: true,
        };

        let json = serde_json::to_value(&original).expect("serialize streaming session DTO");
        let round_tripped: StreamingSessionDto =
            serde_json::from_value(json).expect("deserialize streaming session DTO");

        assert_eq!(round_tripped, original);
    }
}
