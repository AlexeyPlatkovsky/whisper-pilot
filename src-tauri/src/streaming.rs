//! Testable persistence facade behind the Streaming Tauri commands —
//! parallel to `meetings.rs`, matching its "open the store fresh per call"
//! convention rather than caching a connection in `AppState`.

use crate::error::{AppError, Result};
use crate::streaming_store::{NewStreamingSession, StreamingSessionId, StreamingStore};
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
pub struct StreamingSessionDto {
    pub id: StreamingSessionId,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub status: String,
    pub windows: Vec<StreamingWindowDto>,
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
    Ok(StreamingSessionDto {
        id: session.id,
        title: session.title,
        created_at_ms: session.created_at_ms,
        updated_at_ms: session.updated_at_ms,
        status: session.status,
        windows,
    })
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
}
