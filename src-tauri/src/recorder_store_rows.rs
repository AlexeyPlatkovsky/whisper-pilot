//! SQLite row projections used by Recorder persistence.

use crate::recorder_store::RecorderSegment;
use rusqlite::Row;

pub(super) fn segment_from_row(row: &Row<'_>) -> rusqlite::Result<RecorderSegment> {
    Ok(RecorderSegment {
        id: row.get(0)?,
        session_id: row.get(1)?,
        start_sample: row.get(2)?,
        end_sample: row.get(3)?,
        text: row.get(4)?,
        language: row.get(5)?,
    })
}
