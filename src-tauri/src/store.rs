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
            .ok_or_else(|| AppError::Store("new meeting was not found".into()))
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
        require_changed(changed, "meeting", meeting.id)
    }

    pub fn delete_meeting(&self, id: MeetingId) -> Result<()> {
        let changed = self
            .connection()?
            .execute("DELETE FROM meetings WHERE id = ?1", params![id])
            .map_err(store_error)?;
        require_changed(changed, "meeting", id)
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
        if let Some(segment) = segments
            .iter()
            .find(|segment| segment.end_ms < segment.start_ms)
        {
            return Err(AppError::Store(format!(
                "segment {} ends before it starts",
                segment.ordinal
            )));
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(store_error)?;
        if !meeting_exists(&transaction, meeting_id)? {
            return Err(AppError::Store(format!(
                "meeting {meeting_id} was not found"
            )));
        }
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
mod tests {
    use super::*;
    use crate::error::AppError;

    fn draft(title: &str, created_at_ms: i64) -> NewMeeting {
        NewMeeting {
            title: title.to_string(),
            source_path: Some(format!("/recordings/{title}.m4a")),
            source_name: Some(format!("{title}.m4a")),
            created_at_ms,
            duration_ms: Some(42_000),
            language: "ru".to_string(),
            status: "ready".to_string(),
        }
    }

    fn segment(ordinal: i64, start_ms: i64, end_ms: i64) -> NewSegment {
        NewSegment {
            ordinal,
            start_ms,
            end_ms,
            text: format!("segment {ordinal}"),
            speaker_id: Some(ordinal + 1),
        }
    }

    fn mfu(meeting_id: MeetingId) -> MeetingMfu {
        MeetingMfu {
            meeting_id,
            summary: "Summary".to_string(),
            decisions: "Decision".to_string(),
            action_items: "Action".to_string(),
            open_questions: "Question".to_string(),
            participants: "Participant".to_string(),
        }
    }

    #[test]
    fn given_empty_directory_when_creating_meeting_then_it_persists_and_lists() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = Store::open(temp.path()).expect("open database");

        let created = store
            .create_meeting(draft("Planning", 100))
            .expect("create meeting");

        assert_eq!(
            store.get_meeting(created.id).expect("open meeting"),
            Some(created.clone())
        );
        assert_eq!(
            store.list_meetings().expect("list meetings"),
            vec![MeetingSummary {
                id: created.id,
                title: "Planning".to_string(),
                created_at_ms: 100,
                duration_ms: Some(42_000),
                status: "ready".to_string(),
            }]
        );

        let mut renamed = created;
        renamed.title = "Updated planning".to_string();
        renamed.status = "finished".to_string();
        store.update_meeting(&renamed).expect("update meeting");
        assert_eq!(
            store.get_meeting(renamed.id).expect("open updated meeting"),
            Some(renamed)
        );
    }

    #[test]
    fn given_saved_data_when_reopened_then_meeting_segments_mfu_and_newest_summary_persist() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let first_id;
        let second_id;

        {
            let store = Store::open(temp.path()).expect("open database");
            first_id = store
                .create_meeting(draft("Earlier", 100))
                .expect("create first")
                .id;
            second_id = store
                .create_meeting(draft("Later", 200))
                .expect("create second")
                .id;
            store
                .replace_segments(second_id, &[segment(2, 1_000, 2_000), segment(1, 0, 900)])
                .expect("replace segments");
            store.upsert_mfu(&mfu(second_id)).expect("upsert mfu");
        }

        let reopened = Store::open(temp.path()).expect("reopen database");
        assert_eq!(
            reopened
                .list_meetings()
                .expect("list summaries")
                .into_iter()
                .map(|m| m.id)
                .collect::<Vec<_>>(),
            vec![second_id, first_id]
        );
        assert_eq!(
            reopened
                .list_segments(second_id)
                .expect("list segments")
                .into_iter()
                .map(|s| s.ordinal)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            reopened.get_mfu(second_id).expect("get mfu"),
            Some(mfu(second_id))
        );
    }

    #[test]
    fn recovers_mixed_mfu_schema_with_orphaned_legacy_rows() {
        // Decision table: notes-only migrates; mixed schemas preserve current rows, import valid legacy rows, and discard orphans.
        let legacy_only = tempfile::tempdir().expect("legacy-only app support");
        let connection =
            Connection::open(database_path(legacy_only.path())).expect("open legacy db");
        connection
            .execute_batch(
                "CREATE TABLE meetings (id INTEGER PRIMARY KEY, title TEXT NOT NULL, source_path TEXT, source_name TEXT, created_at_ms INTEGER NOT NULL, duration_ms INTEGER, language TEXT NOT NULL, status TEXT NOT NULL);
                 CREATE TABLE notes (meeting_id INTEGER PRIMARY KEY, summary TEXT NOT NULL, decisions TEXT NOT NULL, action_items TEXT NOT NULL, open_questions TEXT NOT NULL, participants TEXT NOT NULL);
                 INSERT INTO meetings VALUES (1, 'Legacy', NULL, NULL, 1, NULL, 'en', 'finished');
                 INSERT INTO notes VALUES (1, 'Legacy summary', 'Legacy decisions', 'Legacy actions', 'Legacy questions', 'Legacy participants');",
            )
            .expect("seed legacy db");
        drop(connection);

        let legacy_store = Store::open(legacy_only.path()).expect("migrate legacy db");
        assert_eq!(
            legacy_store.get_mfu(1).expect("read migrated mfu"),
            Some(MeetingMfu {
                meeting_id: 1,
                summary: "Legacy summary".to_string(),
                decisions: "Legacy decisions".to_string(),
                action_items: "Legacy actions".to_string(),
                open_questions: "Legacy questions".to_string(),
                participants: "Legacy participants".to_string(),
            })
        );
        drop(legacy_store);
        let reopened_legacy = Store::open(legacy_only.path()).expect("repeat legacy migration");
        assert_eq!(
            reopened_legacy
                .get_mfu(1)
                .expect("read reopened legacy mfu"),
            Some(MeetingMfu {
                meeting_id: 1,
                summary: "Legacy summary".to_string(),
                decisions: "Legacy decisions".to_string(),
                action_items: "Legacy actions".to_string(),
                open_questions: "Legacy questions".to_string(),
                participants: "Legacy participants".to_string(),
            })
        );
        drop(reopened_legacy);
        let legacy_notes_exist = Connection::open(database_path(legacy_only.path()))
            .expect("reopen legacy db")
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'notes')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .expect("read legacy schema");
        assert!(!legacy_notes_exist);

        let mixed = tempfile::tempdir().expect("mixed-schema app support");
        let connection = Connection::open(database_path(mixed.path())).expect("open mixed db");
        connection
            .execute_batch(
                "CREATE TABLE meetings (id INTEGER PRIMARY KEY, title TEXT NOT NULL, source_path TEXT, source_name TEXT, created_at_ms INTEGER NOT NULL, duration_ms INTEGER, language TEXT NOT NULL, status TEXT NOT NULL);
                 CREATE TABLE notes (meeting_id INTEGER PRIMARY KEY, summary TEXT NOT NULL, decisions TEXT NOT NULL, action_items TEXT NOT NULL, open_questions TEXT NOT NULL, participants TEXT NOT NULL);
                 CREATE TABLE mfu (meeting_id INTEGER PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE, summary TEXT NOT NULL, decisions TEXT NOT NULL, action_items TEXT NOT NULL, open_questions TEXT NOT NULL, participants TEXT NOT NULL);
                 INSERT INTO meetings VALUES (1, 'Current', NULL, NULL, 1, NULL, 'en', 'finished'), (2, 'Legacy', NULL, NULL, 2, NULL, 'en', 'finished');
                 INSERT INTO mfu VALUES (1, 'Current summary', 'Current decisions', 'Current actions', 'Current questions', 'Current participants');
                 INSERT INTO notes VALUES (1, 'Stale summary', 'Stale decisions', 'Stale actions', 'Stale questions', 'Stale participants'), (2, 'Imported summary', 'Imported decisions', 'Imported actions', 'Imported questions', 'Imported participants'), (3, 'Orphaned summary', 'Orphaned decisions', 'Orphaned actions', 'Orphaned questions', 'Orphaned participants');",
            )
            .expect("seed mixed db");
        drop(connection);

        let mixed_store = Store::open(mixed.path()).expect("recover mixed db");
        assert_eq!(
            mixed_store.get_mfu(1).expect("read current mfu"),
            Some(MeetingMfu {
                meeting_id: 1,
                summary: "Current summary".to_string(),
                decisions: "Current decisions".to_string(),
                action_items: "Current actions".to_string(),
                open_questions: "Current questions".to_string(),
                participants: "Current participants".to_string(),
            })
        );
        assert_eq!(
            mixed_store.get_mfu(2).expect("read imported mfu"),
            Some(MeetingMfu {
                meeting_id: 2,
                summary: "Imported summary".to_string(),
                decisions: "Imported decisions".to_string(),
                action_items: "Imported actions".to_string(),
                open_questions: "Imported questions".to_string(),
                participants: "Imported participants".to_string(),
            })
        );
        assert_eq!(mixed_store.get_mfu(3).expect("read orphaned mfu"), None);
        drop(mixed_store);
        let reopened_mixed = Store::open(mixed.path()).expect("repeat mixed migration");
        assert_eq!(
            reopened_mixed
                .get_mfu(1)
                .expect("read reopened current mfu"),
            Some(MeetingMfu {
                meeting_id: 1,
                summary: "Current summary".to_string(),
                decisions: "Current decisions".to_string(),
                action_items: "Current actions".to_string(),
                open_questions: "Current questions".to_string(),
                participants: "Current participants".to_string(),
            })
        );
        assert_eq!(
            reopened_mixed
                .get_mfu(3)
                .expect("read reopened orphaned mfu"),
            None
        );
        assert_eq!(
            reopened_mixed
                .get_mfu(2)
                .expect("read reopened imported mfu"),
            Some(MeetingMfu {
                meeting_id: 2,
                summary: "Imported summary".to_string(),
                decisions: "Imported decisions".to_string(),
                action_items: "Imported actions".to_string(),
                open_questions: "Imported questions".to_string(),
                participants: "Imported participants".to_string(),
            })
        );
        drop(reopened_mixed);
        let mixed_notes_exist = Connection::open(database_path(mixed.path()))
            .expect("reopen mixed db")
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'notes')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .expect("read mixed schema");
        assert!(!mixed_notes_exist);
    }

    #[test]
    fn given_invalid_or_duplicate_segments_when_replacing_then_store_error_leaves_no_partial_rows()
    {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = Store::open(temp.path()).expect("open database");
        let meeting_id = store
            .create_meeting(draft("Invalid", 100))
            .expect("create meeting")
            .id;

        store
            .replace_segments(meeting_id, &[segment(5, 0, 100)])
            .expect("save initial segment");
        let expected_segments = store
            .list_segments(meeting_id)
            .expect("list initial segment");

        // EP: a reversed range is invalid while a non-negative ordered range is valid.
        let invalid_range = store.replace_segments(meeting_id, &[segment(0, 2_000, 1_000)]);
        assert!(matches!(invalid_range, Err(AppError::Store(_))));
        assert_eq!(
            store.list_segments(meeting_id).expect("list segments"),
            expected_segments
        );

        // EP: duplicate and distinct ordinals are separate persistence classes.
        let duplicate_ordinal =
            store.replace_segments(meeting_id, &[segment(0, 0, 100), segment(0, 200, 300)]);
        assert!(matches!(duplicate_ordinal, Err(AppError::Store(_))));
        assert_eq!(
            store.list_segments(meeting_id).expect("list segments"),
            expected_segments
        );

        // EP: an existing and an unknown meeting id must not share a write path.
        let unknown_meeting = store.replace_segments(meeting_id + 1_000, &[segment(0, 0, 100)]);
        assert!(matches!(unknown_meeting, Err(AppError::Store(_))));
    }

    #[test]
    fn given_meeting_with_dependents_when_deleted_then_segments_and_mfu_are_cascaded() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = Store::open(temp.path()).expect("open database");
        let meeting_id = store
            .create_meeting(draft("Disposable", 100))
            .expect("create meeting")
            .id;
        store
            .replace_segments(meeting_id, &[segment(0, 0, 100)])
            .expect("replace segments");
        store.upsert_mfu(&mfu(meeting_id)).expect("upsert mfu");

        store.delete_meeting(meeting_id).expect("delete meeting");

        assert_eq!(store.get_meeting(meeting_id).expect("get meeting"), None);
        assert!(store
            .list_segments(meeting_id)
            .expect("list segments")
            .is_empty());
        assert_eq!(store.get_mfu(meeting_id).expect("get mfu"), None);
    }

    #[test]
    fn given_saved_mfu_when_deleted_then_only_the_mfu_record_is_removed() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = Store::open(temp.path()).expect("open database");
        let meeting_id = store
            .create_meeting(draft("MFU", 100))
            .expect("create meeting")
            .id;
        store.upsert_mfu(&mfu(meeting_id)).expect("upsert mfu");

        store.delete_mfu(meeting_id).expect("delete mfu");

        assert_eq!(store.get_mfu(meeting_id).expect("get mfu"), None);
        assert!(store
            .get_meeting(meeting_id)
            .expect("get meeting")
            .is_some());
    }
}
