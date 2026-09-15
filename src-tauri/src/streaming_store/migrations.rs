//! Idempotent schema migrations for the Meeting persistence store.

use super::{store_error, Result};
use rusqlite::Connection;

/// Preserve existing pre-MFU Streaming data while replacing the legacy table name.
pub(super) fn migrate_legacy_streaming_notes(connection: &Connection) -> Result<()> {
    let has_legacy = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'streaming_notes')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(store_error)?;
    if has_legacy {
        let has_mfu = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'streaming_mfu')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(store_error)?;
        if has_mfu {
            connection
                .execute_batch(
                    "INSERT INTO streaming_mfu (session_id, summary, decisions, action_items, open_questions, participants)
                     SELECT streaming_notes.session_id, streaming_notes.summary, streaming_notes.decisions,
                            streaming_notes.action_items, streaming_notes.open_questions,
                            streaming_notes.participants
                     FROM streaming_notes
                     WHERE EXISTS (
                         SELECT 1 FROM streaming_sessions
                         WHERE streaming_sessions.id = streaming_notes.session_id
                     )
                       AND NOT EXISTS (
                         SELECT 1 FROM streaming_mfu
                         WHERE streaming_mfu.session_id = streaming_notes.session_id
                     );
                     DROP TABLE streaming_notes;",
                )
                .map_err(store_error)?;
        } else {
            connection
                .execute_batch("ALTER TABLE streaming_notes RENAME TO streaming_mfu;")
                .map_err(store_error)?;
        }
    }
    Ok(())
}

/// Adds `translation_enabled` to a `streaming_sessions` table that predates
/// the column (WP-101), defaulting every existing row to off (`false`).
pub(super) fn migrate_translation_enabled_column(connection: &Connection) -> Result<()> {
    if !table_exists(connection, "streaming_sessions")? {
        return Ok(());
    }
    if !column_exists(connection, "streaming_sessions", "translation_enabled")? {
        connection
            .execute_batch(
                "ALTER TABLE streaming_sessions ADD COLUMN translation_enabled INTEGER NOT NULL DEFAULT 0;",
            )
            .map_err(store_error)?;
    }
    Ok(())
}

pub(super) fn migrate_translation_target_language_column(connection: &Connection) -> Result<()> {
    if !table_exists(connection, "streaming_sessions")? {
        return Ok(());
    }
    if !column_exists(
        connection,
        "streaming_sessions",
        "translation_target_language",
    )? {
        connection
            .execute_batch(
                "ALTER TABLE streaming_sessions
                 ADD COLUMN translation_target_language TEXT NOT NULL DEFAULT 'ru'
                 CHECK(translation_target_language IN ('en', 'ru'));",
            )
            .map_err(store_error)?;

        if table_exists(connection, "streaming_translations")? {
            let translation_columns = table_columns(connection, "streaming_translations")?;
            let can_restore_target = ["session_id", "target_language", "updated_at_ms"]
                .iter()
                .all(|required| translation_columns.iter().any(|column| column == required));
            if can_restore_target {
                connection
                    .execute_batch(
                        "UPDATE streaming_sessions
                         SET translation_target_language = COALESCE(
                             (
                                 SELECT translations.target_language
                                 FROM streaming_translations AS translations
                                 WHERE translations.session_id = streaming_sessions.id
                                   AND translations.target_language IN ('en', 'ru')
                                 ORDER BY translations.updated_at_ms DESC,
                                          translations.target_language ASC
                                 LIMIT 1
                             ),
                             'ru'
                         );",
                    )
                    .map_err(store_error)?;
            }
        }
    }
    Ok(())
}

/// Renames `streaming_translations.paragraph_key` to `window_index` (WP-103),
/// preserving every row's data.
pub(super) fn migrate_translation_window_index_column(connection: &Connection) -> Result<()> {
    if !table_exists(connection, "streaming_translations")? {
        return Ok(());
    }
    if column_exists(connection, "streaming_translations", "paragraph_key")? {
        connection
            .execute_batch(
                "ALTER TABLE streaming_translations RENAME COLUMN paragraph_key TO window_index;",
            )
            .map_err(store_error)?;
    }
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(store_error)
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    Ok(table_columns(connection, table)?
        .iter()
        .any(|name| name == column))
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(store_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(store_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(store_error)?;
    Ok(columns)
}

pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS streaming_sessions (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    status TEXT NOT NULL,
    translation_enabled INTEGER NOT NULL DEFAULT 0,
    translation_target_language TEXT NOT NULL DEFAULT 'ru'
        CHECK(translation_target_language IN ('en', 'ru'))
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

CREATE TABLE IF NOT EXISTS streaming_session_configuration (
    session_id INTEGER PRIMARY KEY REFERENCES streaming_sessions(id) ON DELETE CASCADE,
    engine TEXT NOT NULL CHECK(engine IN ('local', 'cloud')),
    cloud_provider TEXT,
    cloud_model TEXT,
    CHECK(
        (engine = 'local' AND cloud_provider IS NULL AND cloud_model IS NULL)
        OR (engine = 'cloud' AND cloud_provider IS NOT NULL AND cloud_model IS NOT NULL)
    )
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
    window_index INTEGER NOT NULL,
    target_language TEXT NOT NULL,
    source_text TEXT NOT NULL,
    translated_text TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (session_id, window_index, target_language)
);
"#;
