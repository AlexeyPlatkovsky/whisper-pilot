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
use std::time::Duration;

#[path = "streaming_store/helpers.rs"]
mod helpers;
mod migrations;
use helpers::*;
use migrations::{
    migrate_legacy_streaming_notes, migrate_translation_enabled_column,
    migrate_translation_target_language_column, migrate_translation_window_index_column, SCHEMA,
};

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
    /// WP-101: whether Live Translation was left on for this session. This and
    /// the target language survive reopening and an app restart. Defaults to
    /// `false` for both a
    /// brand-new session and one that predates this column (see the
    /// `translation_enabled` migration below).
    pub translation_enabled: bool,
    /// The target column selected for Live Translation. It belongs to the
    /// session so reopening one cannot silently reinterpret persisted rows
    /// using another session's (or the UI default) language.
    pub translation_target_language: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingSessionSummary {
    pub id: StreamingSessionId,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// Captured timeline length, independent of wall-clock update times.
    pub duration_ms: i64,
    pub status: String,
    pub translation_enabled: bool,
    pub translation_target_language: String,
}

/// The engine selected immediately before a session's first capture. This is
/// intentionally a separate, non-secret row: keys stay in Keychain, while a
/// resumed session can still prove which engine/provider/model owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingSessionConfiguration {
    pub engine: String,
    pub cloud_provider: Option<String>,
    pub cloud_model: Option<String>,
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

/// One window's translation into a target language (WP-92; one row per
/// window rather than per paragraph as of WP-103), keyed by `(session_id,
/// window_index, target_language)` — `window_index` is that window's own
/// index, matching `NewStreamingWindow`/`StoredStreamingWindow`'s field of
/// the same name. `source_text` is stored alongside the translation so a
/// caller holding the *current* window text can detect staleness
/// (`is_stale`) without this store needing any window-grouping concept of
/// its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingTranslation {
    pub session_id: StreamingSessionId,
    pub window_index: i64,
    pub target_language: String,
    pub source_text: String,
    pub translated_text: String,
    pub updated_at_ms: i64,
}

impl StreamingTranslation {
    /// A stored translation is stale once the window it was translated from
    /// has changed (e.g. a fail-open retry rewrote its text) — compared
    /// against the *current* text for this `window_index`, which the caller
    /// supplies since this store has no window-grouping concept of its own.
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

// A short connection-local wait keeps ordinary concurrent UI reads from
// failing a write immediately. The result driver adds a few retries around
// a committed window because failing that write would otherwise stop capture.
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const APPEND_WINDOW_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(10),
    Duration::from_millis(25),
    Duration::from_millis(50),
];

impl StreamingStore {
    pub fn open(app_support_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(app_support_dir)?;
        let connection = Connection::open(crate::store::shared_database_path(app_support_dir))
            .map_err(store_error)?;
        connection
            .busy_timeout(SQLITE_BUSY_TIMEOUT)
            .map_err(store_error)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA foreign_keys = ON;",
            )
            .map_err(store_error)?;
        migrate_legacy_streaming_notes(&connection)?;
        migrate_translation_enabled_column(&connection)?;
        migrate_translation_target_language_column(&connection)?;
        migrate_translation_window_index_column(&connection)?;
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
            .ok_or_else(|| AppError::Store("new meeting was not found".into()))
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
        require_changed(changed, "meeting", id)
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
        require_changed(changed, "meeting", id)
    }

    pub fn set_translation_target_language(
        &self,
        id: StreamingSessionId,
        target_language: &str,
    ) -> Result<()> {
        if !matches!(target_language, "en" | "ru") {
            return Err(AppError::Store(format!(
                "unsupported Meeting translation target language: {target_language}"
            )));
        }
        let changed = self
            .connection()?
            .execute(
                "UPDATE streaming_sessions SET translation_target_language = ?1 WHERE id = ?2",
                params![target_language, id],
            )
            .map_err(store_error)?;
        require_changed(changed, "meeting", id)
    }

    /// Stores the engine configuration exactly once, before capture begins.
    /// A prior row means this session has already been started (or at least
    /// prepared to start), so changing provider/model would make a resumed
    /// session's transcript provenance ambiguous.
    pub fn set_session_configuration(
        &self,
        id: StreamingSessionId,
        configuration: &StreamingSessionConfiguration,
    ) -> Result<()> {
        let connection = self.connection()?;
        let status = connection
            .query_row(
                "SELECT status FROM streaming_sessions WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(store_error)?
            .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;
        if status != crate::streaming_store::status::STOPPED {
            return Err(AppError::Capture(
                "cannot change a Meeting's engine while it is active".to_string(),
            ));
        }
        let changed = connection
            .execute(
                "INSERT INTO streaming_session_configuration
                    (session_id, engine, cloud_provider, cloud_model)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    id,
                    configuration.engine,
                    configuration.cloud_provider,
                    configuration.cloud_model,
                ],
            )
            .map_err(|error| match error {
                rusqlite::Error::SqliteFailure(_, Some(message))
                    if message.contains("UNIQUE constraint failed") =>
                {
                    AppError::Capture(
                        "cannot change a Meeting's engine after it has been set".to_string(),
                    )
                }
                other => store_error(other),
            })?;
        debug_assert_eq!(changed, 1);
        Ok(())
    }

    pub fn get_session_configuration(
        &self,
        id: StreamingSessionId,
    ) -> Result<Option<StreamingSessionConfiguration>> {
        self.connection()?
            .query_row(
                "SELECT engine, cloud_provider, cloud_model
                 FROM streaming_session_configuration WHERE session_id = ?1",
                params![id],
                |row| {
                    Ok(StreamingSessionConfiguration {
                        engine: row.get(0)?,
                        cloud_provider: row.get(1)?,
                        cloud_model: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(store_error)
    }

    pub fn delete_session(&self, id: StreamingSessionId) -> Result<()> {
        let changed = self
            .connection()?
            .execute("DELETE FROM streaming_sessions WHERE id = ?1", params![id])
            .map_err(store_error)?;
        require_changed(changed, "meeting", id)
    }

    /// Atomically clears all content derived from captured audio while keeping
    /// the stopped session, its engine configuration, and UI preferences.
    pub fn clear_session_content(&self, id: StreamingSessionId) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let status = transaction
            .query_row(
                "SELECT status FROM streaming_sessions WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(store_error)?
            .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;
        if status != status::STOPPED {
            return Err(AppError::Capture(
                "cannot clear a Meeting while capture is active".to_string(),
            ));
        }
        for table in [
            "streaming_translations",
            "streaming_prettified",
            "streaming_mfu",
            "streaming_segments",
        ] {
            transaction
                .execute(
                    &format!("DELETE FROM {table} WHERE session_id = ?1"),
                    params![id],
                )
                .map_err(store_error)?;
        }
        transaction
            .execute(
                "UPDATE streaming_sessions SET updated_at_ms = created_at_ms WHERE id = ?1",
                params![id],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)
    }

    pub fn list_sessions(&self) -> Result<Vec<StreamingSessionSummary>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, title, created_at_ms, updated_at_ms, status, translation_enabled,
                        translation_target_language,
                        COALESCE((SELECT MAX(end_ms) FROM streaming_segments
                                  WHERE session_id = streaming_sessions.id), 0)
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
        self.append_window_with_retry_delays(session_id, window, now_ms, &[])
    }

    /// Like [`Self::append_window`], but retries only SQLite's transient
    /// `BUSY`/`LOCKED` failures. This is reserved for the live local result
    /// driver: losing a committed window would terminate capture, while all
    /// other callers should receive their storage error directly.
    pub fn append_window_with_retry(
        &self,
        session_id: StreamingSessionId,
        window: &NewStreamingWindow,
        now_ms: i64,
    ) -> Result<()> {
        self.append_window_with_retry_delays(
            session_id,
            window,
            now_ms,
            &APPEND_WINDOW_RETRY_DELAYS,
        )
    }

    fn append_window_with_retry_delays(
        &self,
        session_id: StreamingSessionId,
        window: &NewStreamingWindow,
        now_ms: i64,
        retry_delays: &[Duration],
    ) -> Result<()> {
        let mut connection = self.connection()?;
        if !session_exists(&connection, session_id)? {
            return Err(AppError::Store(format!(
                "meeting {session_id} was not found"
            )));
        }
        for (attempt, delay) in retry_delays
            .iter()
            .copied()
            .chain(std::iter::once(Duration::ZERO))
            .enumerate()
        {
            match append_window_once(&mut connection, session_id, window, now_ms) {
                Ok(()) => return Ok(()),
                Err(error) if is_busy_or_locked(&error) && delay != Duration::ZERO => {
                    log::debug!(
                        "retrying Meeting window persistence after transient SQLite contention; attempt={}",
                        attempt + 1
                    );
                    std::thread::sleep(delay);
                }
                Err(error) => return Err(store_error(error)),
            }
        }
        unreachable!("the retry sequence always includes a final attempt")
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

    /// Cheap pre-inference guard for Live Translation. Failed capture spans
    /// and stale UI text must never consume a local LLM job.
    pub fn translation_source_is_available(
        &self,
        session_id: StreamingSessionId,
        window_index: i64,
        source_text: &str,
    ) -> Result<bool> {
        let exists = self
            .connection()?
            .query_row(
                "SELECT EXISTS (
                    SELECT 1 FROM streaming_segments
                    WHERE session_id = ?1
                      AND window_index = ?2
                      AND outcome_ok = 1
                      AND text = ?3
                )",
                params![session_id, window_index, source_text],
                |row| row.get::<_, i64>(0),
            )
            .map_err(store_error)?;
        Ok(exists != 0)
    }

    pub fn mark_stopped(&self, id: StreamingSessionId, now_ms: i64) -> Result<()> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE streaming_sessions SET status = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![status::STOPPED, now_ms, id],
            )
            .map_err(store_error)?;
        require_changed(changed, "meeting", id)
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
        require_changed(changed, "meeting", id)
    }

    pub fn upsert_mfu(&self, mfu: &StreamingMfu) -> Result<()> {
        let changed = self
            .connection()?
            .execute(
                "INSERT INTO streaming_mfu
                    (session_id, summary, decisions, action_items, open_questions, participants)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6
                 WHERE EXISTS (
                     SELECT 1 FROM streaming_segments
                     WHERE session_id = ?1 AND outcome_ok = 1 AND TRIM(text) <> ''
                 )
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
        if changed == 0 {
            return Err(AppError::Store(format!(
                "meeting {} has no transcript for MFU",
                mfu.session_id
            )));
        }
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
        let changed = self
            .connection()?
            .execute(
                "INSERT INTO streaming_prettified (session_id, text)
                 SELECT ?1, ?2
                 WHERE EXISTS (
                     SELECT 1 FROM streaming_segments
                     WHERE session_id = ?1 AND outcome_ok = 1 AND TRIM(text) <> ''
                 )
                 ON CONFLICT(session_id) DO UPDATE SET text = excluded.text",
                params![session_id, text],
            )
            .map_err(store_error)?;
        if changed == 0 {
            return Err(AppError::Store(format!(
                "meeting {session_id} has no transcript to prettify"
            )));
        }
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

    /// Upserts one window's translation, keyed by `(session_id,
    /// window_index, target_language)` — a repeated call for the same key
    /// (a re-translate after the window's text changed, or a retry)
    /// overwrites in place rather than duplicating, the same idiom
    /// `append_window` uses for `(session_id, window_index)`.
    pub fn upsert_translation(&self, translation: &StreamingTranslation) -> Result<()> {
        self.connection()?
            .execute(
                "INSERT INTO streaming_translations
                    (session_id, window_index, target_language, source_text, translated_text, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(session_id, window_index, target_language) DO UPDATE SET
                    source_text = excluded.source_text,
                    translated_text = excluded.translated_text,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    translation.session_id,
                    translation.window_index,
                    translation.target_language,
                    translation.source_text,
                    translation.translated_text,
                    translation.updated_at_ms,
                ],
            )
            .map_err(store_error)?;
        Ok(())
    }

    /// Atomically persist an inferred translation only while both the
    /// session toggle and source window still match the request that entered
    /// inference. The single INSERT ... SELECT closes the cancellation/source
    /// TOCTOU window without holding a database lock while the model runs.
    pub fn upsert_translation_if_current(
        &self,
        translation: &StreamingTranslation,
    ) -> Result<bool> {
        let changed = self
            .connection()?
            .execute(
                "INSERT INTO streaming_translations
                    (session_id, window_index, target_language, source_text, translated_text, updated_at_ms)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6
                 WHERE EXISTS (
                    SELECT 1
                    FROM streaming_sessions AS session
                    JOIN streaming_segments AS window ON window.session_id = session.id
                    WHERE session.id = ?1
                      AND session.translation_enabled = 1
                      AND window.window_index = ?2
                      AND window.outcome_ok = 1
                      AND window.text = ?4
                 )
                 ON CONFLICT(session_id, window_index, target_language) DO UPDATE SET
                    source_text = excluded.source_text,
                    translated_text = excluded.translated_text,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    translation.session_id,
                    translation.window_index,
                    translation.target_language,
                    translation.source_text,
                    translation.translated_text,
                    translation.updated_at_ms,
                ],
            )
            .map_err(store_error)?;
        Ok(changed == 1)
    }

    /// All stored translations for one session and target language, ordered
    /// by window position.
    pub fn list_translations(
        &self,
        session_id: StreamingSessionId,
        target_language: &str,
    ) -> Result<Vec<StreamingTranslation>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT session_id, window_index, target_language, source_text, translated_text, updated_at_ms
                 FROM streaming_translations
                 WHERE session_id = ?1 AND target_language = ?2
                 ORDER BY window_index ASC",
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

#[cfg(test)]
mod tests;
