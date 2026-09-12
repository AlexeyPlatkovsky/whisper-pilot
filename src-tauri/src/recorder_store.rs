//! Recorder-only SQLite entities and recovery reconciliation.

use crate::error::{AppError, Result};
use crate::recorder_audio::{finalize_partial_caf, read_caf_metadata, RecorderAudioWriter};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub type RecorderSessionId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecorderStatus {
    Recording,
    Finalizing,
    Completed,
    Recoverable,
    DeleteFailed,
}

impl RecorderStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Finalizing => "finalizing",
            Self::Completed => "completed",
            Self::Recoverable => "recoverable",
            Self::DeleteFailed => "delete_failed",
        }
    }

    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "recording" => Ok(Self::Recording),
            "finalizing" => Ok(Self::Finalizing),
            "completed" => Ok(Self::Completed),
            "recoverable" => Ok(Self::Recoverable),
            "delete_failed" => Ok(Self::DeleteFailed),
            other => Err(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("unknown Recorder status: {other}").into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRecorderSession {
    pub title: String,
    pub created_at_ms: i64,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecorderSession {
    pub id: RecorderSessionId,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub sample_rate: u32,
    pub duration_ms: i64,
    pub status: RecorderStatus,
    pub audio_path: PathBuf,
    pub recovery_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecorderSegment {
    pub id: i64,
    pub session_id: RecorderSessionId,
    pub start_sample: u64,
    pub end_sample: u64,
    pub text: String,
    pub language: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecorderTranscriptUpdate {
    Partial {
        text: String,
    },
    Committed {
        start_sample: u64,
        end_sample: u64,
        text: String,
        language: String,
    },
}

pub struct RecorderStore {
    connection: Mutex<Connection>,
    recordings_dir: PathBuf,
}

impl RecorderStore {
    pub fn open(app_support_dir: &Path) -> Result<Self> {
        let store = Self::open_runtime(app_support_dir)?;
        store.reconcile_interrupted_sessions()?;
        Ok(store)
    }

    /// Opens the shared store without interpreting an actively-written
    /// `.partial` as a launch interruption. Production performs reconciliation
    /// once during app setup, then uses this path for live commands/workers.
    pub(crate) fn open_runtime(app_support_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(app_support_dir)?;
        let recordings_dir = app_support_dir.join("recordings");
        std::fs::create_dir_all(&recordings_dir)?;
        let connection = Connection::open(crate::store::shared_database_path(app_support_dir))
            .map_err(store_error)?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(store_error)?;
        connection.execute_batch(SCHEMA).map_err(store_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
            recordings_dir,
        })
    }

    pub fn create_session(&self, session: NewRecorderSession) -> Result<RecorderSession> {
        if session.sample_rate == 0 {
            return Err(AppError::Store(
                "Recorder sample rate must be greater than zero".into(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(store_error)?;
        transaction
            .execute(
                "INSERT INTO recorder_sessions
                    (title, created_at_ms, updated_at_ms, sample_rate, duration_ms, status, audio_path)
                 VALUES (?1, ?2, ?2, ?3, 0, 'recording', '')",
                params![session.title, session.created_at_ms, session.sample_rate],
            )
            .map_err(store_error)?;
        let id = transaction.last_insert_rowid();
        let audio_path = self.recordings_dir.join(format!("{id}.caf"));
        transaction
            .execute(
                "UPDATE recorder_sessions SET audio_path = ?1 WHERE id = ?2",
                params![audio_path.to_string_lossy(), id],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        drop(connection);
        self.get_session(id)?
            .ok_or_else(|| AppError::Store("new Recorder session was not found".into()))
    }

    pub fn get_session(&self, id: RecorderSessionId) -> Result<Option<RecorderSession>> {
        self.connection()?
            .query_row(SESSION_SELECT_BY_ID, params![id], session_from_row)
            .optional()
            .map_err(store_error)
    }

    pub fn list_sessions(&self) -> Result<Vec<RecorderSession>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(&format!(
                "{} ORDER BY updated_at_ms DESC, id DESC",
                SESSION_SELECT_BASE
            ))
            .map_err(store_error)?;
        let sessions = statement
            .query_map([], session_from_row)
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)?;
        Ok(sessions)
    }

    pub fn rename_session(&self, id: RecorderSessionId, title: &str) -> Result<()> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE recorder_sessions SET title = ?1 WHERE id = ?2",
                params![title, id],
            )
            .map_err(store_error)?;
        require_changed(changed, "Recorder session", id)
    }

    pub fn apply_transcript_update(
        &self,
        session_id: RecorderSessionId,
        update: RecorderTranscriptUpdate,
    ) -> Result<Option<RecorderSegment>> {
        let RecorderTranscriptUpdate::Committed {
            start_sample,
            end_sample,
            text,
            language,
        } = update
        else {
            return Ok(None);
        };
        if end_sample < start_sample {
            return Err(AppError::Store(
                "Recorder segment ends before it starts".into(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(store_error)?;
        let sample_rate: u32 = transaction
            .query_row(
                "SELECT sample_rate FROM recorder_sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(store_error)?
            .ok_or_else(|| {
                AppError::Store(format!("Recorder session {session_id} was not found"))
            })?;
        transaction
            .execute(
                "INSERT INTO recorder_segments
                    (session_id, start_sample, end_sample, text, language)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![session_id, start_sample, end_sample, text, language],
            )
            .map_err(store_error)?;
        let id = transaction.last_insert_rowid();
        let duration_ms = end_sample.saturating_mul(1_000) / u64::from(sample_rate);
        transaction
            .execute(
                "UPDATE recorder_sessions
                 SET duration_ms = MAX(duration_ms, ?1), updated_at_ms = MAX(updated_at_ms, created_at_ms + ?1)
                 WHERE id = ?2",
                params![duration_ms, session_id],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(Some(RecorderSegment {
            id,
            session_id,
            start_sample,
            end_sample,
            text,
            language,
        }))
    }

    pub fn list_segments(&self, session_id: RecorderSessionId) -> Result<Vec<RecorderSegment>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, session_id, start_sample, end_sample, text, language
                 FROM recorder_segments WHERE session_id = ?1 ORDER BY start_sample, id",
            )
            .map_err(store_error)?;
        let segments = statement
            .query_map(params![session_id], segment_from_row)
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)?;
        Ok(segments)
    }

    pub fn upsert_polished(&self, session_id: RecorderSessionId, text: &str) -> Result<()> {
        let changed = self
            .connection()?
            .execute(
                "INSERT INTO recorder_polished (session_id, text)
                 VALUES (?1, ?2)
                 ON CONFLICT(session_id) DO UPDATE SET text = excluded.text",
                params![session_id, text],
            )
            .map_err(store_error)?;
        require_changed(changed, "Recorder session", session_id)
    }

    pub fn get_polished(&self, session_id: RecorderSessionId) -> Result<Option<String>> {
        self.connection()?
            .query_row(
                "SELECT text FROM recorder_polished WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(store_error)
    }

    pub fn delete_polished(&self, session_id: RecorderSessionId) -> Result<()> {
        let exists = self.get_session(session_id)?.is_some();
        if !exists {
            return Err(AppError::Store(format!(
                "Recorder session {session_id} was not found"
            )));
        }
        self.connection()?
            .execute(
                "DELETE FROM recorder_polished WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(store_error)?;
        Ok(())
    }

    pub fn update_segment_text(
        &self,
        session_id: RecorderSessionId,
        segment_id: i64,
        text: &str,
    ) -> Result<()> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE recorder_segments SET text = ?1 WHERE id = ?2 AND session_id = ?3",
                params![text, segment_id, session_id],
            )
            .map_err(store_error)?;
        require_changed(changed, "Recorder segment", segment_id)
    }

    pub fn mark_finalizing(&self, id: RecorderSessionId) -> Result<RecorderSession> {
        self.set_status(id, RecorderStatus::Finalizing, None)?;
        self.get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))
    }

    pub fn mark_completed(&self, id: RecorderSessionId) -> Result<RecorderSession> {
        let session = self
            .get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))?;
        let partial = RecorderAudioWriter::partial_path_for(&session.audio_path);
        if !session.audio_path.is_file() || partial.exists() {
            return Err(AppError::Store(
                "Recorder completion requires exactly one atomically finalized audio file".into(),
            ));
        }
        if !matches!(
            session.status,
            RecorderStatus::Finalizing | RecorderStatus::Recoverable
        ) {
            return Err(AppError::Store(format!(
                "Recorder session {id} cannot complete from {:?}",
                session.status
            )));
        }
        let metadata = read_caf_metadata(&session.audio_path)?;
        if metadata.sample_rate != session.sample_rate {
            return Err(AppError::Store(format!(
                "Recorder audio sample rate {} does not match session sample rate {}",
                metadata.sample_rate, session.sample_rate
            )));
        }
        let duration_ms = metadata.frames.saturating_mul(1_000) / u64::from(metadata.sample_rate);
        self.connection()?
            .execute(
                "UPDATE recorder_sessions
                 SET status = 'completed', recovery_reason = NULL, duration_ms = ?1,
                     updated_at_ms = MAX(updated_at_ms, created_at_ms + ?1)
                 WHERE id = ?2",
                params![duration_ms, id],
            )
            .map_err(store_error)?;
        self.get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))
    }

    pub fn recover_session(&self, id: RecorderSessionId) -> Result<RecorderSession> {
        let session = self
            .get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))?;
        if session.status != RecorderStatus::Recoverable {
            return Err(AppError::Store(format!(
                "Recorder session {id} can only be recovered from Recoverable"
            )));
        }
        let partial = RecorderAudioWriter::partial_path_for(&session.audio_path);
        match (session.audio_path.is_file(), partial.is_file()) {
            (true, true) => {
                return Err(AppError::Store(
                    "Recorder recovery found both final and partial audio; delete the session or resolve the files manually".into(),
                ))
            }
            (false, false) => {
                return Err(AppError::Store(
                    "Recorder recovery found no audio artifact; delete the session".into(),
                ))
            }
            (false, true) => {
                finalize_partial_caf(&session.audio_path)?;
            }
            (true, false) => {}
        }
        self.mark_completed(id)
    }

    pub fn mark_recoverable(&self, id: RecorderSessionId, reason: &str) -> Result<RecorderSession> {
        self.set_status(id, RecorderStatus::Recoverable, Some(reason))?;
        self.get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))
    }

    pub fn get_segment(
        &self,
        session_id: RecorderSessionId,
        segment_id: i64,
    ) -> Result<Option<RecorderSegment>> {
        self.connection()?
            .query_row(
                "SELECT id, session_id, start_sample, end_sample, text, language
                 FROM recorder_segments WHERE session_id = ?1 AND id = ?2",
                params![session_id, segment_id],
                segment_from_row,
            )
            .optional()
            .map_err(store_error)
    }

    pub fn delete_session(&self, id: RecorderSessionId) -> Result<()> {
        let session = self
            .get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))?;
        if matches!(
            session.status,
            RecorderStatus::Recording | RecorderStatus::Finalizing
        ) {
            return Err(AppError::Store(format!(
                "Recorder session {id} cannot be deleted while active"
            )));
        }
        let partial = RecorderAudioWriter::partial_path_for(&session.audio_path);
        for path in [&session.audio_path, &partial] {
            if path.exists() {
                if let Err(error) = std::fs::remove_file(path) {
                    let message = format!("Recorder audio cleanup failed: {error}");
                    let _ = self.set_status(id, RecorderStatus::DeleteFailed, Some(&message));
                    return Err(AppError::Io(message));
                }
            }
        }
        let changed = self
            .connection()?
            .execute("DELETE FROM recorder_sessions WHERE id = ?1", params![id])
            .map_err(store_error)?;
        require_changed(changed, "Recorder session", id)
    }

    pub(crate) fn discard_failed_start(&self, id: RecorderSessionId) -> Result<()> {
        let session = self
            .get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))?;
        if session.status != RecorderStatus::Recording {
            return Err(AppError::Store(format!(
                "Recorder session {id} is no longer a failed startup"
            )));
        }
        let partial = RecorderAudioWriter::partial_path_for(&session.audio_path);
        for path in [&session.audio_path, &partial] {
            if path.is_file() {
                std::fs::remove_file(path)?;
            }
        }
        let changed = self
            .connection()?
            .execute("DELETE FROM recorder_sessions WHERE id = ?1", params![id])
            .map_err(store_error)?;
        require_changed(changed, "Recorder session", id)
    }

    fn set_status(
        &self,
        id: RecorderSessionId,
        status: RecorderStatus,
        reason: Option<&str>,
    ) -> Result<()> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE recorder_sessions SET status = ?1, recovery_reason = ?2 WHERE id = ?3",
                params![status.as_str(), reason, id],
            )
            .map_err(store_error)?;
        require_changed(changed, "Recorder session", id)
    }

    fn reconcile_interrupted_sessions(&self) -> Result<()> {
        let sessions = self.list_sessions()?;
        for session in sessions {
            let partial = RecorderAudioWriter::partial_path_for(&session.audio_path);
            let final_exists = session.audio_path.is_file();
            let partial_exists = partial.is_file();
            let final_valid = final_exists
                && read_caf_metadata(&session.audio_path)
                    .is_ok_and(|metadata| metadata.sample_rate == session.sample_rate);
            let mismatch = match session.status {
                RecorderStatus::Completed => !final_valid || partial_exists,
                RecorderStatus::Recording | RecorderStatus::Finalizing => true,
                RecorderStatus::Recoverable | RecorderStatus::DeleteFailed => false,
            };
            if mismatch {
                let reason = match (final_exists, partial_exists) {
                    (true, true) => {
                        "Both finalized and partial Recorder audio were found after interruption"
                    }
                    (true, false) => {
                        "Finalized Recorder audio was found before database completion"
                    }
                    (false, true) => "Partial Recorder audio was preserved after interruption",
                    (false, false) => "Recorder database state has no matching audio artifact",
                };
                self.set_status(session.id, RecorderStatus::Recoverable, Some(reason))?;
            }
        }
        Ok(())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| AppError::Store("Recorder database lock is poisoned".into()))
    }
}

const SESSION_SELECT_BASE: &str =
    "SELECT id, title, created_at_ms, updated_at_ms, sample_rate, duration_ms, status, audio_path, recovery_reason FROM recorder_sessions";
const SESSION_SELECT_BY_ID: &str =
    "SELECT id, title, created_at_ms, updated_at_ms, sample_rate, duration_ms, status, audio_path, recovery_reason FROM recorder_sessions WHERE id = ?1";

fn session_from_row(row: &Row<'_>) -> rusqlite::Result<RecorderSession> {
    let status: String = row.get(6)?;
    Ok(RecorderSession {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at_ms: row.get(2)?,
        updated_at_ms: row.get(3)?,
        sample_rate: row.get(4)?,
        duration_ms: row.get(5)?,
        status: RecorderStatus::parse(&status)?,
        audio_path: PathBuf::from(row.get::<_, String>(7)?),
        recovery_reason: row.get(8)?,
    })
}

fn segment_from_row(row: &Row<'_>) -> rusqlite::Result<RecorderSegment> {
    Ok(RecorderSegment {
        id: row.get(0)?,
        session_id: row.get(1)?,
        start_sample: row.get(2)?,
        end_sample: row.get(3)?,
        text: row.get(4)?,
        language: row.get(5)?,
    })
}

fn require_changed(changed: usize, entity: &str, id: i64) -> Result<()> {
    if changed == 0 {
        Err(AppError::Store(format!("{entity} {id} was not found")))
    } else {
        Ok(())
    }
}

fn store_error(error: rusqlite::Error) -> AppError {
    AppError::Store(error.to_string())
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS recorder_sessions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    title           TEXT NOT NULL,
    created_at_ms   INTEGER NOT NULL,
    updated_at_ms   INTEGER NOT NULL,
    sample_rate     INTEGER NOT NULL CHECK (sample_rate > 0),
    duration_ms     INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL CHECK (status IN ('recording', 'finalizing', 'completed', 'recoverable', 'delete_failed')),
    audio_path      TEXT NOT NULL,
    recovery_reason TEXT
);
CREATE TABLE IF NOT EXISTS recorder_segments (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id   INTEGER NOT NULL REFERENCES recorder_sessions(id) ON DELETE CASCADE,
    start_sample INTEGER NOT NULL CHECK (start_sample >= 0),
    end_sample   INTEGER NOT NULL CHECK (end_sample >= start_sample),
    text         TEXT NOT NULL,
    language     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_recorder_segments_session_start
    ON recorder_segments(session_id, start_sample, id);
CREATE TABLE IF NOT EXISTS recorder_polished (
    session_id INTEGER PRIMARY KEY REFERENCES recorder_sessions(id) ON DELETE CASCADE,
    text       TEXT NOT NULL
);
"#;
