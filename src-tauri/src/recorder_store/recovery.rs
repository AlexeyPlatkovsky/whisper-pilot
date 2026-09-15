//! Startup reconciliation for interrupted Recorder mutations.

use super::{
    clear_quarantine_path, read_caf_metadata, store_error, RecorderAudioWriter, RecorderStatus,
    RecorderStore,
};
use crate::error::Result;
use rusqlite::params;
use std::path::{Path, PathBuf};

impl RecorderStore {
    pub(super) fn reconcile_interrupted_sessions(&self) -> Result<()> {
        self.remove_committed_delete_quarantines()?;
        for session in self.list_sessions()? {
            let partial = RecorderAudioWriter::partial_path_for(&session.audio_path);
            let artifacts = [&session.audio_path, &partial];
            if session.is_draft {
                for original in artifacts {
                    remove_draft_quarantine(&clear_quarantine_path(original));
                }
                continue;
            }
            for original in artifacts {
                restore_session_quarantine(original, &clear_quarantine_path(original));
            }
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
                    (true, true) => "A Recorder continuation was preserved after interruption",
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

    /// Complete only deletion cleanup explicitly recorded in SQLite. A filename
    /// scan would risk treating a user-created file as Recorder-owned.
    fn remove_committed_delete_quarantines(&self) -> Result<()> {
        let quarantines = {
            let connection = self.connection()?;
            let mut statement = connection
                .prepare("SELECT quarantine_path FROM recorder_cleanup_tombstones")
                .map_err(store_error)?;
            let paths = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(store_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(store_error)?;
            paths
        };
        for recorded_path in quarantines {
            let path = PathBuf::from(&recorded_path);
            if !is_managed_quarantine(&self.recordings_dir, &path) {
                log::warn!(
                    "Recorder cleanup tombstone has an unsafe path {}; preserving it",
                    path.display()
                );
                continue;
            }
            if path.exists() {
                if !path.is_file() {
                    log::warn!(
                        "Recorder cleanup quarantine is not a regular file at {}; preserving it",
                        path.display()
                    );
                    continue;
                }
                if let Err(error) = std::fs::remove_file(&path) {
                    log::warn!(
                        "Recorder committed-delete cleanup failed at {}: {error}",
                        path.display()
                    );
                    continue;
                }
            }
            if let Err(error) = self.connection().and_then(|connection| {
                connection
                    .execute(
                        "DELETE FROM recorder_cleanup_tombstones WHERE quarantine_path = ?1",
                        params![recorded_path],
                    )
                    .map_err(store_error)
                    .map(|_| ())
            }) {
                log::warn!(
                    "Recorder committed-delete cleanup finished at {} but could not clear its tombstone: {error}",
                    path.display()
                );
            }
        }
        Ok(())
    }
}

fn is_managed_quarantine(recordings_dir: &Path, quarantine: &Path) -> bool {
    if quarantine.parent() != Some(recordings_dir) {
        return false;
    }
    let Some(file_name) = quarantine.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(original_name) = file_name.strip_suffix(".clearing") else {
        return false;
    };
    let Some(session_name) = original_name
        .strip_suffix(".caf.partial")
        .or_else(|| original_name.strip_suffix(".caf"))
    else {
        return false;
    };
    session_name.parse::<i64>().ok().is_some_and(|id| id > 0)
}

fn remove_draft_quarantine(quarantine: &Path) {
    if quarantine.is_file() {
        if let Err(error) = std::fs::remove_file(quarantine) {
            log::warn!(
                "Recorder draft quarantine cleanup failed at {}: {error}",
                quarantine.display()
            );
        }
    }
}

fn restore_session_quarantine(original: &Path, quarantine: &Path) {
    if quarantine.is_file() && !original.exists() {
        if let Err(error) = std::fs::rename(quarantine, original) {
            log::warn!(
                "Recorder clear quarantine restore failed from {} to {}: {error}",
                quarantine.display(),
                original.display()
            );
        }
    }
}
