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
pub struct StreamingNotesDto {
    pub summary: String,
    pub decisions: String,
    pub action_items: String,
    pub open_questions: String,
    pub participants: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingSessionDto {
    pub id: StreamingSessionId,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub status: String,
    pub windows: Vec<StreamingWindowDto>,
    pub notes: Option<StreamingNotesDto>,
    pub prettified_text: Option<String>,
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
    let notes = store.get_notes(id)?.map(|n| StreamingNotesDto {
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
        notes,
        prettified_text,
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
            "cannot craft notes while a Streaming session is still active".into(),
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

/// Create a new session record for a session about to start capturing.
/// Titled by creation time (matching Meeting's plain default title) — the
/// user can rename it, same as a meeting.
pub fn create_streaming_session(
    app_support_dir: &Path,
    created_at_ms: i64,
) -> Result<StreamingSessionId> {
    let session = StreamingStore::open(app_support_dir)?.create_session(NewStreamingSession {
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
        },
        next_window_index,
    ))
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
        // Deliberately not marked stopped — session stays "active".

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
        store
            .append_window(id, &ok_window(0, "hello"), 200)
            .expect("append window");
        // Deliberately not marked stopped — session stays "active".

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

        assert_eq!(dto.notes, None);
    }

    #[test]
    fn open_streaming_session_includes_notes_when_present() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = StreamingStore::open(temp.path()).expect("open store");
        let id = create_streaming_session(temp.path(), 100).expect("create");
        store
            .upsert_notes(&crate::streaming_store::StreamingNotes {
                session_id: id,
                summary: "Summary.".to_string(),
                decisions: "Decisions.".to_string(),
                action_items: "Actions.".to_string(),
                open_questions: "Questions.".to_string(),
                participants: "Alex".to_string(),
            })
            .expect("upsert notes");

        let dto = open_streaming_session(temp.path(), id).expect("open");

        assert_eq!(
            dto.notes,
            Some(StreamingNotesDto {
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
            notes: Some(StreamingNotesDto {
                summary: "Summary.".to_string(),
                decisions: "Decisions.".to_string(),
                action_items: "Actions.".to_string(),
                open_questions: "Questions.".to_string(),
                participants: "Alex".to_string(),
            }),
            prettified_text: Some("Cleaned transcript.".to_string()),
        };

        let json = serde_json::to_value(&original).expect("serialize streaming session DTO");
        let round_tripped: StreamingSessionDto =
            serde_json::from_value(json).expect("deserialize streaming session DTO");

        assert_eq!(round_tripped, original);
    }
}
