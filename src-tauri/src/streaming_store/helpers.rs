//! SQLite row mapping and write retry primitives for Streaming persistence.

use super::*;

pub(super) fn store_error(error: rusqlite::Error) -> AppError {
    AppError::Store(error.to_string())
}

pub(super) fn require_changed(changed: usize, kind: &str, id: StreamingSessionId) -> Result<()> {
    if changed == 0 {
        return Err(AppError::Store(format!("{kind} {id} was not found")));
    }
    Ok(())
}

pub(super) fn session_exists(connection: &Connection, id: StreamingSessionId) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM streaming_sessions WHERE id = ?1)",
            params![id],
            |row| row.get(0),
        )
        .map_err(store_error)
}

pub(super) fn append_window_once(
    connection: &mut Connection,
    session_id: StreamingSessionId,
    window: &NewStreamingWindow,
    now_ms: i64,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute(
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
    )?;
    let changed = transaction.execute(
        "UPDATE streaming_sessions SET updated_at_ms = ?1 WHERE id = ?2",
        params![now_ms, session_id],
    )?;
    if changed == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    transaction.commit()
}

pub(super) fn is_busy_or_locked(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(error, _)
            if matches!(
                error.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    )
}

pub(super) fn session_by_id(
    connection: &Connection,
    id: StreamingSessionId,
) -> Result<Option<StreamingSessionRecord>> {
    connection
        .query_row(
            "SELECT id, title, created_at_ms, updated_at_ms, status, translation_enabled,
                    translation_target_language
             FROM streaming_sessions WHERE id = ?1",
            params![id],
            session_from_row,
        )
        .optional()
        .map_err(store_error)
}

pub(super) fn session_from_row(row: &Row<'_>) -> rusqlite::Result<StreamingSessionRecord> {
    Ok(StreamingSessionRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at_ms: row.get(2)?,
        updated_at_ms: row.get(3)?,
        status: row.get(4)?,
        translation_enabled: row.get(5)?,
        translation_target_language: row.get(6)?,
    })
}

pub(super) fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<StreamingSessionSummary> {
    Ok(StreamingSessionSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at_ms: row.get(2)?,
        updated_at_ms: row.get(3)?,
        duration_ms: row.get(7)?,
        status: row.get(4)?,
        translation_enabled: row.get(5)?,
        translation_target_language: row.get(6)?,
    })
}

pub(super) fn mfu_from_row(row: &Row<'_>) -> rusqlite::Result<StreamingMfu> {
    Ok(StreamingMfu {
        session_id: row.get(0)?,
        summary: row.get(1)?,
        decisions: row.get(2)?,
        action_items: row.get(3)?,
        open_questions: row.get(4)?,
        participants: row.get(5)?,
    })
}

pub(super) fn window_from_row(row: &Row<'_>) -> rusqlite::Result<StoredStreamingWindow> {
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

pub(super) fn translation_from_row(row: &Row<'_>) -> rusqlite::Result<StreamingTranslation> {
    Ok(StreamingTranslation {
        session_id: row.get(0)?,
        window_index: row.get(1)?,
        target_language: row.get(2)?,
        source_text: row.get(3)?,
        translated_text: row.get(4)?,
        updated_at_ms: row.get(5)?,
    })
}
