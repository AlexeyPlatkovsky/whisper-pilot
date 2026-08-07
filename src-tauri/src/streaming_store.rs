//! Local SQLite persistence for Streaming sessions and their decoded
//! windows — parallel to, but a separate entity from, `store.rs`'s meeting
//! tables (WP-68 D5): a Streaming session has no backing file, a
//! possibly-mixed per-window language, and no diarization, none of which
//! fit the `meetings` table's shape.
//!
//! Windows are **appended one at a time** (`append_window`), not replaced
//! wholesale like `Store::replace_segments` — that incremental save is what
//! makes a session recoverable after a crash/quit, per WP-68's DoD (only the
//! last in-flight window may be lost). `streaming_session.rs`'s decode loop
//! calls this after every `WindowResult`, successful or not (a failed
//! window still advances the session's `updated_at_ms`, so "the session is
//! alive but this window skipped" is distinguishable from "the session
//! stalled").

use crate::error::{AppError, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub type StreamingSessionId = i64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewStreamingSession {
    pub title: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingSessionRecord {
    pub id: StreamingSessionId,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingSessionSummary {
    pub id: StreamingSessionId,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub status: String,
}

/// One decoded window's text, ready to append. `outcome_ok` distinguishes a
/// successful window (`text` is real transcript) from a fail-open skip
/// (`text` is empty, per `streaming_session::WindowResult`'s `Err` case) —
/// stored rather than inferred from `text.is_empty()`, since a genuinely
/// silent window (e.g. no speech in that span) is not an error and must not
/// look like one on replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewStreamingWindow {
    pub window_index: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub language: String,
    pub outcome_ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredStreamingWindow {
    pub session_id: StreamingSessionId,
    pub window_index: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub language: String,
    pub outcome_ok: bool,
}

/// Structured MFU/Craft notes for a Streaming session — parallel to
/// `store::MeetingNotes`, one row per session (upserted on re-Craft).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingNotes {
    pub session_id: StreamingSessionId,
    pub summary: String,
    pub decisions: String,
    pub action_items: String,
    pub open_questions: String,
    pub participants: String,
}

/// Session lifecycle statuses. Plain strings in the schema (matching
/// `store.rs`'s `meetings.status` convention), typed at the call site so a
/// typo can't silently create a fourth status.
pub mod status {
    pub const ACTIVE: &str = "active";
    pub const STOPPED: &str = "stopped";
}

pub struct StreamingStore {
    connection: Mutex<Connection>,
}

impl StreamingStore {
    pub fn open(app_support_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(app_support_dir)?;
        let connection = Connection::open(crate::store::shared_database_path(app_support_dir))
            .map_err(store_error)?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(store_error)?;
        connection.execute_batch(SCHEMA).map_err(store_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn create_session(&self, session: NewStreamingSession) -> Result<StreamingSessionRecord> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO streaming_sessions (title, created_at_ms, updated_at_ms, status)
                 VALUES (?1, ?2, ?2, ?3)",
                params![session.title, session.created_at_ms, status::ACTIVE],
            )
            .map_err(store_error)?;
        let id = connection.last_insert_rowid();
        session_by_id(&connection, id)?
            .ok_or_else(|| AppError::Store("new streaming session was not found".into()))
    }

    pub fn get_session(&self, id: StreamingSessionId) -> Result<Option<StreamingSessionRecord>> {
        let connection = self.connection()?;
        session_by_id(&connection, id)
    }

    pub fn rename_session(&self, id: StreamingSessionId, title: &str) -> Result<()> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE streaming_sessions SET title = ?1 WHERE id = ?2",
                params![title, id],
            )
            .map_err(store_error)?;
        require_changed(changed, "streaming session", id)
    }

    pub fn delete_session(&self, id: StreamingSessionId) -> Result<()> {
        let changed = self
            .connection()?
            .execute("DELETE FROM streaming_sessions WHERE id = ?1", params![id])
            .map_err(store_error)?;
        require_changed(changed, "streaming session", id)
    }

    pub fn list_sessions(&self) -> Result<Vec<StreamingSessionSummary>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, title, created_at_ms, updated_at_ms, status
                 FROM streaming_sessions ORDER BY updated_at_ms DESC, id DESC",
            )
            .map_err(store_error)?;
        let summaries = statement
            .query_map([], summary_from_row)
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)?;
        Ok(summaries)
    }

    /// Append one window, upserting on `window_index` so a retried save
    /// (e.g. after a transient store error) is idempotent rather than
    /// duplicating or erroring. Advances `updated_at_ms` in the same
    /// transaction — a save that touched the window but not the session's
    /// freshness would be a silent half-write.
    pub fn append_window(
        &self,
        session_id: StreamingSessionId,
        window: &NewStreamingWindow,
        now_ms: i64,
    ) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(store_error)?;
        if !session_exists(&transaction, session_id)? {
            return Err(AppError::Store(format!(
                "streaming session {session_id} was not found"
            )));
        }
        transaction
            .execute(
                "INSERT INTO streaming_segments
                    (session_id, window_index, start_ms, end_ms, text, language, outcome_ok)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(session_id, window_index) DO UPDATE SET
                    start_ms = excluded.start_ms,
                    end_ms = excluded.end_ms,
                    text = excluded.text,
                    language = excluded.language,
                    outcome_ok = excluded.outcome_ok",
                params![
                    session_id,
                    window.window_index,
                    window.start_ms,
                    window.end_ms,
                    window.text,
                    window.language,
                    window.outcome_ok,
                ],
            )
            .map_err(store_error)?;
        let changed = transaction
            .execute(
                "UPDATE streaming_sessions SET updated_at_ms = ?1 WHERE id = ?2",
                params![now_ms, session_id],
            )
            .map_err(store_error)?;
        require_changed(changed, "streaming session", session_id)?;
        transaction.commit().map_err(store_error)
    }

    pub fn list_windows(
        &self,
        session_id: StreamingSessionId,
    ) -> Result<Vec<StoredStreamingWindow>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT session_id, window_index, start_ms, end_ms, text, language, outcome_ok
                 FROM streaming_segments WHERE session_id = ?1 ORDER BY window_index ASC",
            )
            .map_err(store_error)?;
        let windows = statement
            .query_map(params![session_id], window_from_row)
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)?;
        Ok(windows)
    }

    pub fn mark_stopped(&self, id: StreamingSessionId, now_ms: i64) -> Result<()> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE streaming_sessions SET status = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![status::STOPPED, now_ms, id],
            )
            .map_err(store_error)?;
        require_changed(changed, "streaming session", id)
    }

    pub fn upsert_notes(&self, notes: &StreamingNotes) -> Result<()> {
        self.connection()?
            .execute(
                "INSERT INTO streaming_notes
                    (session_id, summary, decisions, action_items, open_questions, participants)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(session_id) DO UPDATE SET
                    summary = excluded.summary,
                    decisions = excluded.decisions,
                    action_items = excluded.action_items,
                    open_questions = excluded.open_questions,
                    participants = excluded.participants",
                params![
                    notes.session_id,
                    notes.summary,
                    notes.decisions,
                    notes.action_items,
                    notes.open_questions,
                    notes.participants,
                ],
            )
            .map_err(store_error)?;
        Ok(())
    }

    pub fn get_notes(&self, session_id: StreamingSessionId) -> Result<Option<StreamingNotes>> {
        self.connection()?
            .query_row(
                "SELECT session_id, summary, decisions, action_items, open_questions, participants
                 FROM streaming_notes WHERE session_id = ?1",
                params![session_id],
                notes_from_row,
            )
            .optional()
            .map_err(store_error)
    }

    pub fn delete_notes(&self, session_id: StreamingSessionId) -> Result<()> {
        self.connection()?
            .execute(
                "DELETE FROM streaming_notes WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(store_error)?;
        Ok(())
    }

    pub fn upsert_prettified(&self, session_id: StreamingSessionId, text: &str) -> Result<()> {
        self.connection()?
            .execute(
                "INSERT INTO streaming_prettified (session_id, text)
                 VALUES (?1, ?2)
                 ON CONFLICT(session_id) DO UPDATE SET text = excluded.text",
                params![session_id, text],
            )
            .map_err(store_error)?;
        Ok(())
    }

    pub fn get_prettified(&self, session_id: StreamingSessionId) -> Result<Option<String>> {
        self.connection()?
            .query_row(
                "SELECT text FROM streaming_prettified WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(store_error)
    }

    pub fn delete_prettified(&self, session_id: StreamingSessionId) -> Result<()> {
        self.connection()?
            .execute(
                "DELETE FROM streaming_prettified WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(store_error)?;
        Ok(())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| AppError::Store("streaming database connection lock was poisoned".into()))
    }
}

fn store_error(error: rusqlite::Error) -> AppError {
    AppError::Store(error.to_string())
}

fn require_changed(changed: usize, kind: &str, id: StreamingSessionId) -> Result<()> {
    if changed == 0 {
        return Err(AppError::Store(format!("{kind} {id} was not found")));
    }
    Ok(())
}

fn session_exists(connection: &Connection, id: StreamingSessionId) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM streaming_sessions WHERE id = ?1)",
            params![id],
            |row| row.get(0),
        )
        .map_err(store_error)
}

fn session_by_id(
    connection: &Connection,
    id: StreamingSessionId,
) -> Result<Option<StreamingSessionRecord>> {
    connection
        .query_row(
            "SELECT id, title, created_at_ms, updated_at_ms, status
             FROM streaming_sessions WHERE id = ?1",
            params![id],
            session_from_row,
        )
        .optional()
        .map_err(store_error)
}

fn session_from_row(row: &Row<'_>) -> rusqlite::Result<StreamingSessionRecord> {
    Ok(StreamingSessionRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at_ms: row.get(2)?,
        updated_at_ms: row.get(3)?,
        status: row.get(4)?,
    })
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<StreamingSessionSummary> {
    Ok(StreamingSessionSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at_ms: row.get(2)?,
        updated_at_ms: row.get(3)?,
        status: row.get(4)?,
    })
}

fn notes_from_row(row: &Row<'_>) -> rusqlite::Result<StreamingNotes> {
    Ok(StreamingNotes {
        session_id: row.get(0)?,
        summary: row.get(1)?,
        decisions: row.get(2)?,
        action_items: row.get(3)?,
        open_questions: row.get(4)?,
        participants: row.get(5)?,
    })
}

fn window_from_row(row: &Row<'_>) -> rusqlite::Result<StoredStreamingWindow> {
    Ok(StoredStreamingWindow {
        session_id: row.get(0)?,
        window_index: row.get(1)?,
        start_ms: row.get(2)?,
        end_ms: row.get(3)?,
        text: row.get(4)?,
        language: row.get(5)?,
        outcome_ok: row.get(6)?,
    })
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS streaming_sessions (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    status TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS streaming_segments (
    session_id INTEGER NOT NULL REFERENCES streaming_sessions(id) ON DELETE CASCADE,
    window_index INTEGER NOT NULL,
    start_ms INTEGER NOT NULL CHECK(start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK(end_ms >= start_ms),
    text TEXT NOT NULL,
    language TEXT NOT NULL,
    outcome_ok INTEGER NOT NULL,
    PRIMARY KEY (session_id, window_index)
);

CREATE TABLE IF NOT EXISTS streaming_notes (
    session_id INTEGER PRIMARY KEY REFERENCES streaming_sessions(id) ON DELETE CASCADE,
    summary TEXT NOT NULL,
    decisions TEXT NOT NULL,
    action_items TEXT NOT NULL,
    open_questions TEXT NOT NULL,
    participants TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS streaming_prettified (
    session_id INTEGER PRIMARY KEY REFERENCES streaming_sessions(id) ON DELETE CASCADE,
    text TEXT NOT NULL
);
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    fn draft(title: &str, created_at_ms: i64) -> NewStreamingSession {
        NewStreamingSession {
            title: title.to_string(),
            created_at_ms,
        }
    }

    fn window(window_index: i64, start_ms: i64, end_ms: i64) -> NewStreamingWindow {
        NewStreamingWindow {
            window_index,
            start_ms,
            end_ms,
            text: format!("window {window_index}"),
            language: "en".to_string(),
            outcome_ok: true,
        }
    }

    #[test]
    fn given_empty_directory_when_creating_session_then_it_persists_and_lists() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");

        let created = store
            .create_session(draft("Standup", 100))
            .expect("create session");

        assert_eq!(created.status, status::ACTIVE);
        assert_eq!(
            store.get_session(created.id).expect("get session"),
            Some(created.clone())
        );
        assert_eq!(
            store.list_sessions().expect("list sessions"),
            vec![StreamingSessionSummary {
                id: created.id,
                title: "Standup".to_string(),
                created_at_ms: 100,
                updated_at_ms: 100,
                status: status::ACTIVE.to_string(),
            }]
        );
    }

    #[test]
    fn given_saved_windows_when_reopened_then_session_and_windows_persist_in_order() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let session_id;

        {
            let store = StreamingStore::open(temp.path()).expect("open database");
            session_id = store
                .create_session(draft("Live thoughts", 100))
                .expect("create session")
                .id;
            store
                .append_window(session_id, &window(1, 7_000, 14_000), 14_500)
                .expect("append window 1");
            store
                .append_window(session_id, &window(0, 0, 7_000), 7_500)
                .expect("append window 0");
        }

        let reopened = StreamingStore::open(temp.path()).expect("reopen database");
        assert_eq!(
            reopened
                .list_windows(session_id)
                .expect("list windows")
                .into_iter()
                .map(|w| w.window_index)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        // The session's updated_at_ms reflects the last append, not creation.
        assert_eq!(
            reopened
                .get_session(session_id)
                .expect("get session")
                .map(|s| s.updated_at_ms),
            Some(7_500)
        );
    }

    #[test]
    fn appending_the_same_window_index_twice_overwrites_rather_than_duplicates() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store
            .create_session(draft("Retry", 100))
            .expect("create session")
            .id;

        store
            .append_window(session_id, &window(0, 0, 7_000), 7_100)
            .expect("first append");
        let mut retried = window(0, 0, 7_000);
        retried.text = "corrected text".to_string();
        store
            .append_window(session_id, &retried, 7_200)
            .expect("retried append (idempotent upsert)");

        let windows = store.list_windows(session_id).expect("list windows");
        assert_eq!(windows.len(), 1, "retry must overwrite, not duplicate");
        assert_eq!(windows[0].text, "corrected text");
    }

    #[test]
    fn a_failed_window_is_stored_with_outcome_ok_false_not_indistinguishable_silence() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store
            .create_session(draft("Fail-open", 100))
            .expect("create session")
            .id;

        let failed = NewStreamingWindow {
            window_index: 0,
            start_ms: 0,
            end_ms: 7_000,
            text: String::new(),
            language: "auto".to_string(),
            outcome_ok: false,
        };
        store
            .append_window(session_id, &failed, 7_100)
            .expect("append failed window");

        let windows = store.list_windows(session_id).expect("list windows");
        assert!(!windows[0].outcome_ok);
    }

    #[test]
    fn appending_to_an_unknown_session_is_a_store_error_not_a_silent_insert() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");

        let result = store.append_window(999_999, &window(0, 0, 7_000), 100);

        assert!(matches!(result, Err(AppError::Store(_))));
    }

    #[test]
    fn given_session_with_windows_when_deleted_then_windows_are_cascaded() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store
            .create_session(draft("Disposable", 100))
            .expect("create session")
            .id;
        store
            .append_window(session_id, &window(0, 0, 7_000), 7_100)
            .expect("append window");

        store.delete_session(session_id).expect("delete session");

        assert_eq!(store.get_session(session_id).expect("get session"), None);
        assert!(store
            .list_windows(session_id)
            .expect("list windows")
            .is_empty());
    }

    #[test]
    fn renaming_an_unknown_session_is_a_store_error() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");

        let result = store.rename_session(999_999, "New title");

        assert!(matches!(result, Err(AppError::Store(_))));
    }

    #[test]
    fn marking_a_session_stopped_updates_status_and_freshness() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store
            .create_session(draft("Ends", 100))
            .expect("create session")
            .id;

        store.mark_stopped(session_id, 5_000).expect("mark stopped");

        let session = store
            .get_session(session_id)
            .expect("get session")
            .expect("session exists");
        assert_eq!(session.status, status::STOPPED);
        assert_eq!(session.updated_at_ms, 5_000);
    }

    #[test]
    fn two_streaming_sessions_have_independent_window_sequences() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let a = store.create_session(draft("A", 100)).unwrap().id;
        let b = store.create_session(draft("B", 200)).unwrap().id;

        store.append_window(a, &window(0, 0, 7_000), 7_100).unwrap();
        store.append_window(b, &window(0, 0, 7_000), 7_100).unwrap();

        assert_eq!(store.list_windows(a).unwrap().len(), 1);
        assert_eq!(store.list_windows(b).unwrap().len(), 1);
    }

    fn notes(session_id: StreamingSessionId) -> StreamingNotes {
        StreamingNotes {
            session_id,
            summary: "Discussed Q3 roadmap.".to_string(),
            decisions: "Ship M1 by Friday.".to_string(),
            action_items: "Alex: update deck".to_string(),
            open_questions: "Budget for Q4?".to_string(),
            participants: "Alex, Sam".to_string(),
        }
    }

    #[test]
    fn given_no_notes_when_getting_then_result_is_none() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

        assert_eq!(store.get_notes(session_id).expect("get notes"), None);
    }

    #[test]
    fn upserted_notes_round_trip() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

        store
            .upsert_notes(&notes(session_id))
            .expect("upsert notes");

        assert_eq!(
            store.get_notes(session_id).expect("get notes"),
            Some(notes(session_id))
        );
    }

    #[test]
    fn upserting_notes_twice_overwrites_rather_than_duplicates() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store
            .upsert_notes(&notes(session_id))
            .expect("first upsert");

        let mut second = notes(session_id);
        second.summary = "Revised summary.".to_string();
        store.upsert_notes(&second).expect("second upsert");

        assert_eq!(
            store.get_notes(session_id).expect("get notes"),
            Some(second)
        );
    }

    #[test]
    fn upserting_notes_for_a_nonexistent_session_is_a_store_error() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");

        // The streaming_notes.session_id foreign key rejects this without
        // any application-level existence check needed.
        assert!(store.upsert_notes(&notes(999_999)).is_err());
    }

    #[test]
    fn deleting_a_session_cascades_its_notes() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store
            .upsert_notes(&notes(session_id))
            .expect("upsert notes");

        store.delete_session(session_id).expect("delete session");

        assert_eq!(store.get_notes(session_id).expect("get notes"), None);
    }

    #[test]
    fn deleting_notes_directly_leaves_the_session_intact() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store
            .upsert_notes(&notes(session_id))
            .expect("upsert notes");

        store.delete_notes(session_id).expect("delete notes");

        assert_eq!(store.get_notes(session_id).expect("get notes"), None);
        assert!(store
            .get_session(session_id)
            .expect("get session")
            .is_some());
    }

    #[test]
    fn given_no_prettified_text_when_getting_then_result_is_none() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

        assert_eq!(
            store.get_prettified(session_id).expect("get prettified"),
            None
        );
    }

    #[test]
    fn upserted_prettified_text_round_trips() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

        store
            .upsert_prettified(session_id, "Cleaned transcript text.")
            .expect("upsert prettified");

        assert_eq!(
            store.get_prettified(session_id).expect("get prettified"),
            Some("Cleaned transcript text.".to_string())
        );
    }

    #[test]
    fn upserting_prettified_text_twice_overwrites_rather_than_duplicates() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store
            .upsert_prettified(session_id, "First version.")
            .expect("first upsert");

        store
            .upsert_prettified(session_id, "Revised version.")
            .expect("second upsert");

        assert_eq!(
            store.get_prettified(session_id).expect("get prettified"),
            Some("Revised version.".to_string())
        );
    }

    #[test]
    fn upserting_prettified_text_for_a_nonexistent_session_is_a_store_error() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");

        assert!(store.upsert_prettified(999_999, "text").is_err());
    }

    #[test]
    fn deleting_a_session_cascades_its_prettified_text() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store
            .upsert_prettified(session_id, "Cleaned text.")
            .expect("upsert prettified");

        store.delete_session(session_id).expect("delete session");

        assert_eq!(
            store.get_prettified(session_id).expect("get prettified"),
            None
        );
    }

    #[test]
    fn deleting_prettified_text_directly_leaves_the_session_intact() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store
            .upsert_prettified(session_id, "Cleaned text.")
            .expect("upsert prettified");

        store
            .delete_prettified(session_id)
            .expect("delete prettified");

        assert_eq!(
            store.get_prettified(session_id).expect("get prettified"),
            None
        );
        assert!(store
            .get_session(session_id)
            .expect("get session")
            .is_some());
    }
}
