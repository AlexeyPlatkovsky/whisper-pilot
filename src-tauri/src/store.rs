//! Local SQLite persistence for meetings, transcript segments, and mfu.

use crate::error::{AppError, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

const DATABASE_FILE_NAME: &str = "whisperpilot.sqlite3";

/// `streaming_store.rs` opens its own connection to this same file (SQLite
/// supports multiple connections to one file); shared so there is exactly
/// one name for "the app's database" rather than two constants that could
/// drift apart.
pub(crate) fn shared_database_path(app_support_dir: &Path) -> PathBuf {
    database_path(app_support_dir)
}

pub type MeetingId = i64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMeeting {
    pub title: String,
    pub source_path: Option<String>,
    pub source_name: Option<String>,
    pub created_at_ms: i64,
    pub duration_ms: Option<i64>,
    pub language: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meeting {
    pub id: MeetingId,
    pub title: String,
    pub source_path: Option<String>,
    pub source_name: Option<String>,
    pub created_at_ms: i64,
    pub duration_ms: Option<i64>,
    pub language: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingSummary {
    pub id: MeetingId,
    pub title: String,
    pub created_at_ms: i64,
    pub duration_ms: Option<i64>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSegment {
    pub ordinal: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub speaker_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSegment {
    pub meeting_id: MeetingId,
    pub ordinal: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub speaker_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MeetingMfu {
    pub meeting_id: MeetingId,
    pub summary: String,
    pub decisions: String,
    pub action_items: String,
    pub open_questions: String,
    pub participants: String,
}

pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    pub fn open(app_support_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(app_support_dir)?;
        let connection = Connection::open(database_path(app_support_dir)).map_err(store_error)?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(store_error)?;
        migrate_legacy_notes(&connection)?;
        connection.execute_batch(SCHEMA).map_err(store_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn create_meeting(&self, meeting: NewMeeting) -> Result<Meeting> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO meetings (title, source_path, source_name, created_at_ms, duration_ms, language, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    meeting.title,
                    meeting.source_path,
                    meeting.source_name,
                    meeting.created_at_ms,
                    meeting.duration_ms,
                    meeting.language,
                    meeting.status,
                ],
            )
            .map_err(store_error)?;
        let id = connection.last_insert_rowid();
        meeting_by_id(&connection, id)?
            .ok_or_else(|| AppError::Store("new transcription was not found".into()))
    }

    pub fn get_meeting(&self, id: MeetingId) -> Result<Option<Meeting>> {
        let connection = self.connection()?;
        meeting_by_id(&connection, id)
    }

    pub fn update_meeting(&self, meeting: &Meeting) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE meetings
                 SET title = ?1, source_path = ?2, source_name = ?3, created_at_ms = ?4,
                     duration_ms = ?5, language = ?6, status = ?7
                 WHERE id = ?8",
                params![
                    meeting.title,
                    meeting.source_path,
                    meeting.source_name,
                    meeting.created_at_ms,
                    meeting.duration_ms,
                    meeting.language,
                    meeting.status,
                    meeting.id,
                ],
            )
            .map_err(store_error)?;
        require_changed(changed, "transcription", meeting.id)
    }

    pub fn delete_meeting(&self, id: MeetingId) -> Result<()> {
        let changed = self
            .connection()?
            .execute("DELETE FROM meetings WHERE id = ?1", params![id])
            .map_err(store_error)?;
        require_changed(changed, "transcription", id)
    }

    /// Atomically removes transcript-derived data while retaining the meeting
    /// row and its attached source, ready for a later re-transcription.
    pub fn clear_meeting_content(&self, id: MeetingId, language: &str) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let changed = transaction
            .execute(
                "UPDATE meetings
                 SET duration_ms = NULL, language = ?1,
                     status = CASE WHEN source_path IS NULL THEN 'no_files' ELSE 'ready' END
                 WHERE id = ?2",
                params![language, id],
            )
            .map_err(store_error)?;
        require_changed(changed, "transcription", id)?;
        transaction
            .execute("DELETE FROM segments WHERE meeting_id = ?1", params![id])
            .map_err(store_error)?;
        transaction
            .execute("DELETE FROM mfu WHERE meeting_id = ?1", params![id])
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)
    }

    pub fn list_meetings(&self) -> Result<Vec<MeetingSummary>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, title, created_at_ms, duration_ms, status
                 FROM meetings ORDER BY created_at_ms DESC, id DESC",
            )
            .map_err(store_error)?;
        let summaries = statement
            .query_map([], summary_from_row)
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)?;
        Ok(summaries)
    }

    pub fn replace_segments(&self, meeting_id: MeetingId, segments: &[NewSegment]) -> Result<()> {
        validate_segments(segments)?;

        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(store_error)?;
        if !meeting_exists(&transaction, meeting_id)? {
            return Err(AppError::Store(format!(
                "transcription {meeting_id} was not found"
            )));
        }
        replace_segments_in_transaction(&transaction, meeting_id, segments)?;
        transaction.commit().map_err(store_error)
    }

    /// Replace the transcript and mark its parent transcription complete in
    /// one SQLite transaction. A failed metadata update must never leave a
    /// newly-replaced transcript under stale meeting metadata.
    pub fn complete_transcript(&self, meeting: &Meeting, segments: &[NewSegment]) -> Result<()> {
        validate_segments(segments)?;

        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(store_error)?;
        if !meeting_exists(&transaction, meeting.id)? {
            return Err(AppError::Store(format!(
                "transcription {} was not found",
                meeting.id
            )));
        }
        replace_segments_in_transaction(&transaction, meeting.id, segments)?;
        update_meeting_in_transaction(&transaction, meeting)?;
        transaction.commit().map_err(store_error)
    }

    pub fn list_segments(&self, meeting_id: MeetingId) -> Result<Vec<StoredSegment>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT meeting_id, ordinal, start_ms, end_ms, text, speaker_id
                 FROM segments WHERE meeting_id = ?1 ORDER BY ordinal ASC",
            )
            .map_err(store_error)?;
        let segments = statement
            .query_map(params![meeting_id], segment_from_row)
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)?;
        Ok(segments)
    }

    pub fn upsert_mfu(&self, mfu: &MeetingMfu) -> Result<()> {
        self.connection()?
            .execute(
                "INSERT INTO mfu (meeting_id, summary, decisions, action_items, open_questions, participants)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(meeting_id) DO UPDATE SET
                    summary = excluded.summary,
                    decisions = excluded.decisions,
                    action_items = excluded.action_items,
                    open_questions = excluded.open_questions,
                    participants = excluded.participants",
                params![
                    mfu.meeting_id,
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

    pub fn get_mfu(&self, meeting_id: MeetingId) -> Result<Option<MeetingMfu>> {
        self.connection()?
            .query_row(
                "SELECT meeting_id, summary, decisions, action_items, open_questions, participants
                 FROM mfu WHERE meeting_id = ?1",
                params![meeting_id],
                mfu_from_row,
            )
            .optional()
            .map_err(store_error)
    }

    pub fn delete_mfu(&self, meeting_id: MeetingId) -> Result<()> {
        self.connection()?
            .execute("DELETE FROM mfu WHERE meeting_id = ?1", params![meeting_id])
            .map_err(store_error)?;
        Ok(())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| AppError::Store("database connection lock was poisoned".into()))
    }
}

/// Preserve existing pre-MFU meeting data while replacing the legacy table name.
fn migrate_legacy_notes(connection: &Connection) -> Result<()> {
    let has_legacy = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'notes')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(store_error)?;
    if has_legacy {
        let has_mfu = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'mfu')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(store_error)?;
        if has_mfu {
            connection
                .execute_batch(
                    "INSERT INTO mfu (meeting_id, summary, decisions, action_items, open_questions, participants)
                     SELECT notes.meeting_id, notes.summary, notes.decisions, notes.action_items,
                            notes.open_questions, notes.participants
                     FROM notes
                     WHERE EXISTS (SELECT 1 FROM meetings WHERE meetings.id = notes.meeting_id)
                       AND NOT EXISTS (SELECT 1 FROM mfu WHERE mfu.meeting_id = notes.meeting_id);
                     DROP TABLE notes;",
                )
                .map_err(store_error)?;
        } else {
            connection
                .execute_batch("ALTER TABLE notes RENAME TO mfu;")
                .map_err(store_error)?;
        }
    }
    Ok(())
}

fn database_path(app_support_dir: &Path) -> PathBuf {
    app_support_dir.join(DATABASE_FILE_NAME)
}

fn store_error(error: rusqlite::Error) -> AppError {
    AppError::Store(error.to_string())
}

fn require_changed(changed: usize, kind: &str, id: MeetingId) -> Result<()> {
    if changed == 0 {
        return Err(AppError::Store(format!("{kind} {id} was not found")));
    }
    Ok(())
}

fn validate_segments(segments: &[NewSegment]) -> Result<()> {
    if let Some(segment) = segments
        .iter()
        .find(|segment| segment.end_ms < segment.start_ms)
    {
        return Err(AppError::Store(format!(
            "segment {} ends before it starts",
            segment.ordinal
        )));
    }
    Ok(())
}

fn replace_segments_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    meeting_id: MeetingId,
    segments: &[NewSegment],
) -> Result<()> {
    transaction
        .execute(
            "DELETE FROM segments WHERE meeting_id = ?1",
            params![meeting_id],
        )
        .map_err(store_error)?;
    for segment in segments {
        transaction
            .execute(
                "INSERT INTO segments (meeting_id, ordinal, start_ms, end_ms, text, speaker_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    meeting_id,
                    segment.ordinal,
                    segment.start_ms,
                    segment.end_ms,
                    segment.text,
                    segment.speaker_id,
                ],
            )
            .map_err(store_error)?;
    }
    Ok(())
}

fn update_meeting_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    meeting: &Meeting,
) -> Result<()> {
    let changed = transaction
        .execute(
            "UPDATE meetings
             SET title = ?1, source_path = ?2, source_name = ?3, created_at_ms = ?4,
                 duration_ms = ?5, language = ?6, status = ?7
             WHERE id = ?8",
            params![
                meeting.title,
                meeting.source_path,
                meeting.source_name,
                meeting.created_at_ms,
                meeting.duration_ms,
                meeting.language,
                meeting.status,
                meeting.id,
            ],
        )
        .map_err(store_error)?;
    require_changed(changed, "transcription", meeting.id)
}

fn meeting_exists(connection: &Connection, meeting_id: MeetingId) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM meetings WHERE id = ?1)",
            params![meeting_id],
            |row| row.get(0),
        )
        .map_err(store_error)
}

fn meeting_by_id(connection: &Connection, id: MeetingId) -> Result<Option<Meeting>> {
    connection
        .query_row(
            "SELECT id, title, source_path, source_name, created_at_ms, duration_ms, language, status
             FROM meetings WHERE id = ?1",
            params![id],
            meeting_from_row,
        )
        .optional()
        .map_err(store_error)
}

fn meeting_from_row(row: &Row<'_>) -> rusqlite::Result<Meeting> {
    Ok(Meeting {
        id: row.get(0)?,
        title: row.get(1)?,
        source_path: row.get(2)?,
        source_name: row.get(3)?,
        created_at_ms: row.get(4)?,
        duration_ms: row.get(5)?,
        language: row.get(6)?,
        status: row.get(7)?,
    })
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<MeetingSummary> {
    Ok(MeetingSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at_ms: row.get(2)?,
        duration_ms: row.get(3)?,
        status: row.get(4)?,
    })
}

fn segment_from_row(row: &Row<'_>) -> rusqlite::Result<StoredSegment> {
    Ok(StoredSegment {
        meeting_id: row.get(0)?,
        ordinal: row.get(1)?,
        start_ms: row.get(2)?,
        end_ms: row.get(3)?,
        text: row.get(4)?,
        speaker_id: row.get(5)?,
    })
}

fn mfu_from_row(row: &Row<'_>) -> rusqlite::Result<MeetingMfu> {
    Ok(MeetingMfu {
        meeting_id: row.get(0)?,
        summary: row.get(1)?,
        decisions: row.get(2)?,
        action_items: row.get(3)?,
        open_questions: row.get(4)?,
        participants: row.get(5)?,
    })
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meetings (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    source_path TEXT,
    source_name TEXT,
    created_at_ms INTEGER NOT NULL,
    duration_ms INTEGER,
    language TEXT NOT NULL,
    status TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS segments (
    meeting_id INTEGER NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    start_ms INTEGER NOT NULL CHECK(start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK(end_ms >= start_ms),
    text TEXT NOT NULL,
    speaker_id INTEGER,
    PRIMARY KEY (meeting_id, ordinal)
);

CREATE TABLE IF NOT EXISTS mfu (
    meeting_id INTEGER PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
    summary TEXT NOT NULL,
    decisions TEXT NOT NULL,
    action_items TEXT NOT NULL,
    open_questions TEXT NOT NULL,
    participants TEXT NOT NULL
);
"#;

#[cfg(test)]
#[path = "store/tests.rs"]
mod tests;
