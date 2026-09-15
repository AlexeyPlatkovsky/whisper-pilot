//! Transactional Recorder artifact clearing.

use super::{
    clear_quarantine_path, restore_quarantined_audio, store_error, RecorderAudioWriter,
    RecorderSessionId, RecorderStore,
};
use crate::error::{AppError, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use std::path::PathBuf;

/// Clear every recording artifact while retaining its named library row as an empty draft.
/// Existing audio is first renamed to a same-volume quarantine and restored if the database
/// transaction fails.
pub(super) fn clear_recording(store: &RecorderStore, session_id: RecorderSessionId) -> Result<()> {
    let mut connection = store.connection()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(store_error)?;
    let (status, audio_path) = transaction
        .query_row(
            "SELECT status, audio_path FROM recorder_sessions WHERE id = ?1",
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    PathBuf::from(row.get::<_, String>(1)?),
                ))
            },
        )
        .optional()
        .map_err(store_error)?
        .ok_or_else(|| AppError::Store(format!("Recorder session {session_id} was not found")))?;
    if matches!(status.as_str(), "recording" | "finalizing") {
        return Err(AppError::Store(format!(
            "Recorder session {session_id} cannot be cleared while capture is active"
        )));
    }

    let partial = RecorderAudioWriter::partial_path_for(&audio_path);
    let artifacts = [audio_path, partial];
    for path in &artifacts {
        if path.exists() && !path.is_file() {
            return Err(AppError::Io(format!(
                "Recorder audio cleanup failed: {} is not a file",
                path.display()
            )));
        }
        let quarantine = clear_quarantine_path(path);
        if quarantine.exists() {
            return Err(AppError::Io(format!(
                "Recorder audio cleanup is already pending at {}",
                quarantine.display()
            )));
        }
    }

    let mut quarantined = Vec::with_capacity(artifacts.len());
    for original in artifacts {
        if !original.exists() {
            continue;
        }
        let quarantine = clear_quarantine_path(&original);
        if let Err(error) = std::fs::rename(&original, &quarantine) {
            let restore = restore_quarantined_audio(&quarantined);
            let detail = restore.err().map_or_else(String::new, |restore_error| {
                format!("; rollback also failed: {restore_error}")
            });
            return Err(AppError::Io(format!(
                "Recorder audio cleanup failed: {error}{detail}"
            )));
        }
        quarantined.push((original, quarantine));
    }

    let database_result = (|| -> Result<()> {
        transaction
            .execute(
                "DELETE FROM recorder_polished WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "DELETE FROM recorder_segments WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(store_error)?;
        transaction
            .execute(
                "UPDATE recorder_sessions
                 SET status = 'completed', is_draft = 1, duration_ms = 0,
                     recovery_reason = NULL
                 WHERE id = ?1",
                params![session_id],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)
    })();
    if let Err(error) = database_result {
        return match restore_quarantined_audio(&quarantined) {
            Ok(()) => Err(error),
            Err(restore_error) => Err(AppError::Io(format!(
                "{error}; Recorder audio rollback also failed: {restore_error}"
            ))),
        };
    }

    for (_, quarantine) in quarantined {
        if let Err(error) = std::fs::remove_file(&quarantine) {
            log::warn!(
                "Recorder clear committed but quarantine cleanup failed at {}: {error}",
                quarantine.display()
            );
        }
    }
    Ok(())
}
