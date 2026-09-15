//! Recorder SQLite schema and additive migrations.

use crate::error::{AppError, Result};
use rusqlite::Connection;

pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS recorder_sessions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    title           TEXT NOT NULL,
    created_at_ms   INTEGER NOT NULL,
    updated_at_ms   INTEGER NOT NULL,
    sample_rate     INTEGER NOT NULL CHECK (sample_rate > 0),
    duration_ms     INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL CHECK (status IN ('recording', 'finalizing', 'completed', 'recoverable', 'delete_failed')),
    audio_path      TEXT NOT NULL,
    recovery_reason TEXT,
    asr_model_id   TEXT NOT NULL DEFAULT 'transcription',
    asr_engine     TEXT NOT NULL DEFAULT 'whisper',
    asr_language   TEXT NOT NULL DEFAULT 'auto',
    is_draft       INTEGER NOT NULL DEFAULT 0 CHECK (is_draft IN (0, 1))
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
-- Kept independently of recorder_sessions so a committed deletion can finish
-- removing same-volume quarantine files after a crash or process termination.
CREATE TABLE IF NOT EXISTS recorder_cleanup_tombstones (
    quarantine_path TEXT PRIMARY KEY
);
"#;

pub(super) fn migrate(connection: &Connection) -> Result<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(recorder_sessions)")
        .map_err(store_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(store_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(store_error)?;
    drop(statement);
    for (column, sql) in [
        ("asr_model_id", "ALTER TABLE recorder_sessions ADD COLUMN asr_model_id TEXT NOT NULL DEFAULT 'transcription'"),
        ("asr_engine", "ALTER TABLE recorder_sessions ADD COLUMN asr_engine TEXT NOT NULL DEFAULT 'whisper'"),
        ("asr_language", "ALTER TABLE recorder_sessions ADD COLUMN asr_language TEXT NOT NULL DEFAULT 'auto'"),
        ("is_draft", "ALTER TABLE recorder_sessions ADD COLUMN is_draft INTEGER NOT NULL DEFAULT 0 CHECK (is_draft IN (0, 1))"),
    ] {
        if !columns.iter().any(|existing| existing == column) {
            connection.execute(sql, []).map_err(store_error)?;
        }
    }
    Ok(())
}

fn store_error(error: rusqlite::Error) -> AppError {
    AppError::Store(error.to_string())
}
