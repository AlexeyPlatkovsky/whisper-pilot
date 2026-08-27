//! Local SQLite persistence for Streaming sessions and their decoded
//! windows — a separate entity from `store.rs`'s meeting tables (WP-68 D5).
//! Windows are appended one at a time (`append_window`), not replaced
//! wholesale like `Store::replace_segments` — that incremental save is what
//! makes a session recoverable after a crash/quit. See
//! docs/architecture.md's Streaming Persistence section for the full
//! entity-shape and recovery-contract rationale.

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
    /// WP-101: whether Live Translation was left on for this session — unlike
    /// the target language (WP-99, never persisted), this survives reopening
    /// the session and an app restart. Defaults to `false` for both a
    /// brand-new session and one that predates this column (see the
    /// `translation_enabled` migration below).
    pub translation_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingSessionSummary {
    pub id: StreamingSessionId,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub status: String,
    pub translation_enabled: bool,
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

/// Structured MFU/Craft MFU for a Streaming session — parallel to
/// `store::MeetingMfu`, one row per session (upserted on re-Craft).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingMfu {
    pub session_id: StreamingSessionId,
    pub summary: String,
    pub decisions: String,
    pub action_items: String,
    pub open_questions: String,
    pub participants: String,
}

/// One paragraph's translation into a target language (WP-92), keyed by
/// `(session_id, paragraph_key, target_language)` — `paragraph_key` is the
/// `window_index` of the paragraph's first window, a stable anchor the
/// front-end's paragraph grouping (`src/paragraphs.ts`) does not otherwise
/// expose to this store. `source_text` is stored alongside the translation
/// so a caller holding the *current* paragraph text can detect staleness
/// (`is_stale`) without this store needing any paragraph concept of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingTranslation {
    pub session_id: StreamingSessionId,
    pub paragraph_key: i64,
    pub target_language: String,
    pub source_text: String,
    pub translated_text: String,
    pub updated_at_ms: i64,
}

impl StreamingTranslation {
    /// A stored translation is stale once the paragraph it was translated
    /// from has changed (e.g. a later window arrived and extended the
    /// paragraph) — compared against the *current* paragraph text for this
    /// `paragraph_key`, which the caller supplies since this store has no
    /// paragraph concept of its own.
    pub fn is_stale(&self, current_source_text: &str) -> bool {
        self.source_text != current_source_text
    }
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
        migrate_legacy_streaming_notes(&connection)?;
        migrate_translation_enabled_column(&connection)?;
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
                params![session.title, session.created_at_ms, status::STOPPED],
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

    /// Persists the Live Translation on/off choice for one session (WP-101),
    /// mirroring `rename_session`'s single-field-update shape and error
    /// handling for an unknown session id.
    pub fn set_translation_enabled(&self, id: StreamingSessionId, enabled: bool) -> Result<()> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE streaming_sessions SET translation_enabled = ?1 WHERE id = ?2",
                params![enabled, id],
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
                "SELECT id, title, created_at_ms, updated_at_ms, status, translation_enabled
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

    /// Inverse of `mark_stopped` — flips a stopped session back to active so
    /// capture can resume into it. Callers are responsible for confirming the
    /// session is actually `STOPPED` first; this does not itself validate the
    /// prior status.
    pub fn mark_active(&self, id: StreamingSessionId, now_ms: i64) -> Result<()> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE streaming_sessions SET status = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![status::ACTIVE, now_ms, id],
            )
            .map_err(store_error)?;
        require_changed(changed, "streaming session", id)
    }

    pub fn upsert_mfu(&self, mfu: &StreamingMfu) -> Result<()> {
        self.connection()?
            .execute(
                "INSERT INTO streaming_mfu
                    (session_id, summary, decisions, action_items, open_questions, participants)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(session_id) DO UPDATE SET
                    summary = excluded.summary,
                    decisions = excluded.decisions,
                    action_items = excluded.action_items,
                    open_questions = excluded.open_questions,
                    participants = excluded.participants",
                params![
                    mfu.session_id,
                    mfu.summary,
                    mfu.decisions,
                    mfu.action_items,
                    mfu.open_questions,
                    mfu.participants,
                ],
            )
            .map_err(store_error)?;
        Ok(())
    }

    pub fn get_mfu(&self, session_id: StreamingSessionId) -> Result<Option<StreamingMfu>> {
        self.connection()?
            .query_row(
                "SELECT session_id, summary, decisions, action_items, open_questions, participants
                 FROM streaming_mfu WHERE session_id = ?1",
                params![session_id],
                mfu_from_row,
            )
            .optional()
            .map_err(store_error)
    }

    pub fn delete_mfu(&self, session_id: StreamingSessionId) -> Result<()> {
        self.connection()?
            .execute(
                "DELETE FROM streaming_mfu WHERE session_id = ?1",
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

    /// Upserts one paragraph's translation, keyed by `(session_id,
    /// paragraph_key, target_language)` — a repeated call for the same key
    /// (a re-translate after the paragraph changed, or a retry) overwrites
    /// in place rather than duplicating, the same idiom `append_window` uses
    /// for `(session_id, window_index)`.
    pub fn upsert_translation(&self, translation: &StreamingTranslation) -> Result<()> {
        self.connection()?
            .execute(
                "INSERT INTO streaming_translations
                    (session_id, paragraph_key, target_language, source_text, translated_text, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(session_id, paragraph_key, target_language) DO UPDATE SET
                    source_text = excluded.source_text,
                    translated_text = excluded.translated_text,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    translation.session_id,
                    translation.paragraph_key,
                    translation.target_language,
                    translation.source_text,
                    translation.translated_text,
                    translation.updated_at_ms,
                ],
            )
            .map_err(store_error)?;
        Ok(())
    }

    /// All stored translations for one session and target language, ordered
    /// by paragraph position.
    pub fn list_translations(
        &self,
        session_id: StreamingSessionId,
        target_language: &str,
    ) -> Result<Vec<StreamingTranslation>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT session_id, paragraph_key, target_language, source_text, translated_text, updated_at_ms
                 FROM streaming_translations
                 WHERE session_id = ?1 AND target_language = ?2
                 ORDER BY paragraph_key ASC",
            )
            .map_err(store_error)?;
        let translations = statement
            .query_map(params![session_id, target_language], translation_from_row)
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)?;
        Ok(translations)
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

/// Preserve existing pre-MFU Streaming data while replacing the legacy table name.
fn migrate_legacy_streaming_notes(connection: &Connection) -> Result<()> {
    let has_legacy = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'streaming_notes')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(store_error)?;
    if has_legacy {
        connection
            .execute_batch("ALTER TABLE streaming_notes RENAME TO streaming_mfu;")
            .map_err(store_error)?;
    }
    Ok(())
}

/// Adds `translation_enabled` to a `streaming_sessions` table that predates
/// the column (WP-101), defaulting every existing row to off (`false`).
/// SQLite's `ALTER TABLE ... ADD COLUMN` has no `IF NOT EXISTS` clause, so —
/// unlike the other tables here, which fold new tables into the idempotent
/// `CREATE TABLE IF NOT EXISTS` schema batch — this checks the column's
/// presence via `PRAGMA table_info` first, the same check-then-alter shape
/// `migrate_legacy_streaming_notes` uses. A no-op on a brand-new database
/// (no `streaming_sessions` table yet: `CREATE TABLE IF NOT EXISTS` below
/// already creates it with the column) and on a database that already has
/// the column (this feature's own migration, run on a prior launch).
fn migrate_translation_enabled_column(connection: &Connection) -> Result<()> {
    let has_table = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'streaming_sessions')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(store_error)?;
    if !has_table {
        return Ok(());
    }
    let has_column = {
        let mut statement = connection
            .prepare("PRAGMA table_info(streaming_sessions)")
            .map_err(store_error)?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)?;
        columns.iter().any(|name| name == "translation_enabled")
    };
    if !has_column {
        connection
            .execute_batch(
                "ALTER TABLE streaming_sessions ADD COLUMN translation_enabled INTEGER NOT NULL DEFAULT 0;",
            )
            .map_err(store_error)?;
    }
    Ok(())
}

fn session_by_id(
    connection: &Connection,
    id: StreamingSessionId,
) -> Result<Option<StreamingSessionRecord>> {
    connection
        .query_row(
            "SELECT id, title, created_at_ms, updated_at_ms, status, translation_enabled
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
        translation_enabled: row.get(5)?,
    })
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<StreamingSessionSummary> {
    Ok(StreamingSessionSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at_ms: row.get(2)?,
        updated_at_ms: row.get(3)?,
        status: row.get(4)?,
        translation_enabled: row.get(5)?,
    })
}

fn mfu_from_row(row: &Row<'_>) -> rusqlite::Result<StreamingMfu> {
    Ok(StreamingMfu {
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

fn translation_from_row(row: &Row<'_>) -> rusqlite::Result<StreamingTranslation> {
    Ok(StreamingTranslation {
        session_id: row.get(0)?,
        paragraph_key: row.get(1)?,
        target_language: row.get(2)?,
        source_text: row.get(3)?,
        translated_text: row.get(4)?,
        updated_at_ms: row.get(5)?,
    })
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS streaming_sessions (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    status TEXT NOT NULL,
    translation_enabled INTEGER NOT NULL DEFAULT 0
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

CREATE TABLE IF NOT EXISTS streaming_mfu (
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

CREATE TABLE IF NOT EXISTS streaming_translations (
    session_id INTEGER NOT NULL REFERENCES streaming_sessions(id) ON DELETE CASCADE,
    paragraph_key INTEGER NOT NULL,
    target_language TEXT NOT NULL,
    source_text TEXT NOT NULL,
    translated_text TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (session_id, paragraph_key, target_language)
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

        assert_eq!(created.status, status::STOPPED);
        // WP-101: a newly created session defaults translation_enabled to
        // false — nothing has been persisted for it yet.
        assert!(!created.translation_enabled);
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
                status: status::STOPPED.to_string(),
                translation_enabled: false,
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

    fn mfu(session_id: StreamingSessionId) -> StreamingMfu {
        StreamingMfu {
            session_id,
            summary: "Discussed Q3 roadmap.".to_string(),
            decisions: "Ship M1 by Friday.".to_string(),
            action_items: "Alex: update deck".to_string(),
            open_questions: "Budget for Q4?".to_string(),
            participants: "Alex, Sam".to_string(),
        }
    }

    #[test]
    fn given_no_mfu_when_getting_then_result_is_none() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

        assert_eq!(store.get_mfu(session_id).expect("get mfu"), None);
    }

    #[test]
    fn marking_a_stopped_session_active_again_updates_status_and_freshness() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store
            .create_session(draft("Resumable", 100))
            .expect("create session")
            .id;
        store.mark_stopped(session_id, 5_000).expect("mark stopped");

        store.mark_active(session_id, 9_000).expect("mark active");

        let session = store
            .get_session(session_id)
            .expect("get session")
            .expect("session exists");
        assert_eq!(session.status, status::ACTIVE);
        assert_eq!(session.updated_at_ms, 9_000);
    }

    #[test]
    fn marking_an_unknown_session_active_is_a_store_error() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");

        let result = store.mark_active(999_999, 100);

        assert!(matches!(result, Err(AppError::Store(_))));
    }

    #[test]
    fn upserted_mfu_round_trip() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

        store.upsert_mfu(&mfu(session_id)).expect("upsert mfu");

        assert_eq!(
            store.get_mfu(session_id).expect("get mfu"),
            Some(mfu(session_id))
        );
    }

    #[test]
    fn upserting_mfu_twice_overwrites_rather_than_duplicates() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store.upsert_mfu(&mfu(session_id)).expect("first upsert");

        let mut second = mfu(session_id);
        second.summary = "Revised summary.".to_string();
        store.upsert_mfu(&second).expect("second upsert");

        assert_eq!(store.get_mfu(session_id).expect("get mfu"), Some(second));
    }

    #[test]
    fn upserting_mfu_for_a_nonexistent_session_is_a_store_error() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");

        // The streaming_mfu.session_id foreign key rejects this without
        // any application-level existence check needed.
        assert!(store.upsert_mfu(&mfu(999_999)).is_err());
    }

    #[test]
    fn deleting_a_session_cascades_its_mfu() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store.upsert_mfu(&mfu(session_id)).expect("upsert mfu");

        store.delete_session(session_id).expect("delete session");

        assert_eq!(store.get_mfu(session_id).expect("get mfu"), None);
    }

    #[test]
    fn deleting_mfu_directly_leaves_the_session_intact() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store.upsert_mfu(&mfu(session_id)).expect("upsert mfu");

        store.delete_mfu(session_id).expect("delete mfu");

        assert_eq!(store.get_mfu(session_id).expect("get mfu"), None);
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

    // --- WP-92: streaming_translations ---

    fn translation(
        session_id: StreamingSessionId,
        paragraph_key: i64,
        target_language: &str,
    ) -> StreamingTranslation {
        StreamingTranslation {
            session_id,
            paragraph_key,
            target_language: target_language.to_string(),
            source_text: "Привет, мир.".to_string(),
            translated_text: "Hello, world.".to_string(),
            updated_at_ms: 1_000,
        }
    }

    #[test]
    fn given_no_translations_when_listing_then_result_is_empty() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

        assert!(store
            .list_translations(session_id, "en")
            .expect("list translations")
            .is_empty());
    }

    #[test]
    fn upserted_translation_round_trips_through_list() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

        store
            .upsert_translation(&translation(session_id, 0, "en"))
            .expect("upsert translation");

        assert_eq!(
            store.list_translations(session_id, "en").expect("list"),
            vec![translation(session_id, 0, "en")]
        );
    }

    #[test]
    fn upserting_the_same_paragraph_key_and_language_twice_overwrites_rather_than_duplicates() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store
            .upsert_translation(&translation(session_id, 0, "en"))
            .expect("first upsert");

        let mut revised = translation(session_id, 0, "en");
        revised.translated_text = "Hello, everyone.".to_string();
        revised.updated_at_ms = 2_000;
        store
            .upsert_translation(&revised)
            .expect("second upsert (retranslate)");

        let rows = store.list_translations(session_id, "en").expect("list");
        assert_eq!(rows.len(), 1, "retranslation must overwrite, not duplicate");
        assert_eq!(rows[0].translated_text, "Hello, everyone.");
        assert_eq!(rows[0].updated_at_ms, 2_000);
    }

    #[test]
    fn translations_for_different_target_languages_are_independent_rows() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

        store
            .upsert_translation(&translation(session_id, 0, "en"))
            .expect("upsert en");
        store
            .upsert_translation(&translation(session_id, 0, "ru"))
            .expect("upsert ru");

        assert_eq!(store.list_translations(session_id, "en").unwrap().len(), 1);
        assert_eq!(store.list_translations(session_id, "ru").unwrap().len(), 1);
    }

    #[test]
    fn translations_for_different_sessions_are_independent() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let a = store.create_session(draft("A", 100)).unwrap().id;
        let b = store.create_session(draft("B", 200)).unwrap().id;

        store
            .upsert_translation(&translation(a, 0, "en"))
            .expect("upsert for a");

        assert_eq!(store.list_translations(a, "en").unwrap().len(), 1);
        assert!(store.list_translations(b, "en").unwrap().is_empty());
    }

    #[test]
    fn upserting_a_translation_for_a_nonexistent_session_is_a_store_error() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");

        let result = store.upsert_translation(&translation(999_999, 0, "en"));

        assert!(matches!(result, Err(AppError::Store(_))));
    }

    #[test]
    fn deleting_a_session_cascades_its_translations() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Disposable", 100)).unwrap().id;
        store
            .upsert_translation(&translation(session_id, 0, "en"))
            .expect("upsert translation");

        store.delete_session(session_id).expect("delete session");

        assert!(store
            .list_translations(session_id, "en")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn streaming_translation_reports_stale_when_source_text_no_longer_matches() {
        let stored = translation(1, 0, "en");
        assert!(stored.is_stale("Привет, мир! (изменено)"));
    }

    #[test]
    fn streaming_translation_reports_not_stale_when_source_text_still_matches() {
        let stored = translation(1, 0, "en");
        assert!(!stored.is_stale("Привет, мир."));
    }

    /// Opening a database that predates the `streaming_translations` table
    /// (but already has sessions/segments/mfu/prettified data) must both
    /// preserve that existing data and make the new table usable —
    /// `CREATE TABLE IF NOT EXISTS` migration, same shape as the
    /// `streaming_prettified` precedent.
    #[test]
    fn opening_a_pre_migration_database_preserves_existing_data_and_adds_translations_table() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let db_path = crate::store::shared_database_path(temp.path());
        std::fs::create_dir_all(temp.path()).expect("create app-support dir");

        {
            // Build a pre-WP-92 database by hand: every table this feature's
            // migration must leave intact, deliberately excluding
            // streaming_translations.
            let connection = Connection::open(&db_path).expect("open raw pre-migration database");
            connection
                .execute_batch(
                    r#"
                    CREATE TABLE streaming_sessions (
                        id INTEGER PRIMARY KEY,
                        title TEXT NOT NULL,
                        created_at_ms INTEGER NOT NULL,
                        updated_at_ms INTEGER NOT NULL,
                        status TEXT NOT NULL
                    );
                    CREATE TABLE streaming_segments (
                        session_id INTEGER NOT NULL REFERENCES streaming_sessions(id) ON DELETE CASCADE,
                        window_index INTEGER NOT NULL,
                        start_ms INTEGER NOT NULL CHECK(start_ms >= 0),
                        end_ms INTEGER NOT NULL CHECK(end_ms >= start_ms),
                        text TEXT NOT NULL,
                        language TEXT NOT NULL,
                        outcome_ok INTEGER NOT NULL,
                        PRIMARY KEY (session_id, window_index)
                    );
                    CREATE TABLE streaming_mfu (
                        session_id INTEGER PRIMARY KEY REFERENCES streaming_sessions(id) ON DELETE CASCADE,
                        summary TEXT NOT NULL,
                        decisions TEXT NOT NULL,
                        action_items TEXT NOT NULL,
                        open_questions TEXT NOT NULL,
                        participants TEXT NOT NULL
                    );
                    CREATE TABLE streaming_prettified (
                        session_id INTEGER PRIMARY KEY REFERENCES streaming_sessions(id) ON DELETE CASCADE,
                        text TEXT NOT NULL
                    );

                    INSERT INTO streaming_sessions (id, title, created_at_ms, updated_at_ms, status)
                        VALUES (1, 'Pre-migration session', 100, 200, 'stopped');
                    INSERT INTO streaming_segments
                        (session_id, window_index, start_ms, end_ms, text, language, outcome_ok)
                        VALUES (1, 0, 0, 7000, 'hello there', 'en', 1);
                    INSERT INTO streaming_mfu
                        (session_id, summary, decisions, action_items, open_questions, participants)
                        VALUES (1, 'Summary.', 'Decisions.', 'Actions.', 'Questions.', 'Alex');
                    INSERT INTO streaming_prettified (session_id, text)
                        VALUES (1, 'Cleaned transcript.');
                    "#,
                )
                .expect("seed pre-migration schema and data");
        }

        let store = StreamingStore::open(temp.path()).expect("open (and migrate) database");

        // Pre-existing data across every prior streaming table survived.
        let session = store
            .get_session(1)
            .expect("get session")
            .expect("session survives migration");
        assert_eq!(session.title, "Pre-migration session");
        assert_eq!(session.status, status::STOPPED);

        let windows = store.list_windows(1).expect("list windows");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].text, "hello there");

        assert_eq!(
            store.get_mfu(1).expect("get mfu").map(|m| m.summary),
            Some("Summary.".to_string())
        );
        assert_eq!(
            store.get_prettified(1).expect("get prettified"),
            Some("Cleaned transcript.".to_string())
        );

        // The new table exists and is immediately usable.
        assert!(store.list_translations(1, "en").expect("list").is_empty());
        store
            .upsert_translation(&translation(1, 0, "en"))
            .expect("upsert into migrated table");
        assert_eq!(store.list_translations(1, "en").expect("list").len(), 1);
    }

    // --- WP-101: translation_enabled column, migration, and persistence ---

    /// Opening a database whose `streaming_sessions` table predates the
    /// `translation_enabled` column must both preserve the existing session
    /// row and add the column, defaulted to false (off) — the same
    /// check-then-`ALTER TABLE` shape as `migrate_legacy_streaming_notes`,
    /// since SQLite's `ADD COLUMN` has no `IF NOT EXISTS` clause to fold into
    /// the idempotent `CREATE TABLE IF NOT EXISTS` schema batch.
    #[test]
    fn opening_a_pre_migration_database_adds_translation_enabled_defaulted_to_false() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let db_path = crate::store::shared_database_path(temp.path());
        std::fs::create_dir_all(temp.path()).expect("create app-support dir");

        {
            // A pre-WP-101 streaming_sessions table: every column this
            // feature's migration must leave intact, deliberately excluding
            // translation_enabled.
            let connection = Connection::open(&db_path).expect("open raw pre-migration database");
            connection
                .execute_batch(
                    r#"
                    CREATE TABLE streaming_sessions (
                        id INTEGER PRIMARY KEY,
                        title TEXT NOT NULL,
                        created_at_ms INTEGER NOT NULL,
                        updated_at_ms INTEGER NOT NULL,
                        status TEXT NOT NULL
                    );
                    INSERT INTO streaming_sessions (id, title, created_at_ms, updated_at_ms, status)
                        VALUES (1, 'Pre-migration session', 100, 200, 'stopped');
                    "#,
                )
                .expect("seed pre-migration schema and data");
        }

        let store = StreamingStore::open(temp.path()).expect("open (and migrate) database");

        let session = store
            .get_session(1)
            .expect("get session")
            .expect("session survives migration");
        assert_eq!(session.title, "Pre-migration session");
        assert_eq!(session.status, status::STOPPED);
        assert!(
            !session.translation_enabled,
            "a pre-existing session must default to translation_enabled = false"
        );

        // The column is now writable, not just readable with a default.
        store
            .set_translation_enabled(1, true)
            .expect("set translation_enabled on the migrated column");
        assert!(
            store
                .get_session(1)
                .expect("get session")
                .expect("session exists")
                .translation_enabled
        );
    }

    /// Reopening the (already-migrated) database a second time must not
    /// error on a duplicate `ALTER TABLE ADD COLUMN` — the column-presence
    /// check must make the migration a no-op once the column exists.
    #[test]
    fn reopening_an_already_migrated_database_does_not_error() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        {
            let store = StreamingStore::open(temp.path()).expect("first open");
            store.create_session(draft("Standup", 100)).unwrap();
        }

        StreamingStore::open(temp.path()).expect("reopening an already-migrated database");
    }

    #[test]
    fn set_translation_enabled_persists_and_is_readable_after_reopen() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let session_id;
        {
            let store = StreamingStore::open(temp.path()).expect("open database");
            session_id = store.create_session(draft("Standup", 100)).unwrap().id;
            assert!(
                !store
                    .get_session(session_id)
                    .unwrap()
                    .unwrap()
                    .translation_enabled
            );

            store
                .set_translation_enabled(session_id, true)
                .expect("set translation_enabled");
        }

        let reopened = StreamingStore::open(temp.path()).expect("reopen database");
        assert!(
            reopened
                .get_session(session_id)
                .expect("get session")
                .expect("session exists")
                .translation_enabled
        );
    }

    #[test]
    fn set_translation_enabled_back_to_false_overwrites_the_prior_value() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store
            .set_translation_enabled(session_id, true)
            .expect("enable");

        store
            .set_translation_enabled(session_id, false)
            .expect("disable");

        assert!(
            !store
                .get_session(session_id)
                .expect("get session")
                .expect("session exists")
                .translation_enabled
        );
    }

    #[test]
    fn setting_translation_enabled_for_an_unknown_session_is_a_store_error() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");

        let result = store.set_translation_enabled(999_999, true);

        assert!(matches!(result, Err(AppError::Store(_))));
    }

    #[test]
    fn two_sessions_have_independent_translation_enabled_values() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = StreamingStore::open(temp.path()).expect("open database");
        let a = store.create_session(draft("A", 100)).unwrap().id;
        let b = store.create_session(draft("B", 200)).unwrap().id;

        store.set_translation_enabled(a, true).expect("enable a");

        assert!(store.get_session(a).unwrap().unwrap().translation_enabled);
        assert!(!store.get_session(b).unwrap().unwrap().translation_enabled);
    }
}
