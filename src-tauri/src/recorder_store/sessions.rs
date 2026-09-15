//! Recorder session creation, resumption, and failed-start restoration.

use super::{
    require_changed, store_error, NewRecorderSession, RecorderSession, RecorderSessionId,
    RecorderStatus, RecorderStore,
};
use crate::error::{AppError, Result};
use crate::recorder_audio::{read_caf_metadata, RecorderAudioWriter};
use rusqlite::params;

impl RecorderStore {
    pub fn create_session(&self, session: NewRecorderSession) -> Result<RecorderSession> {
        self.insert_session(session, false)
    }

    pub fn create_draft(&self, session: NewRecorderSession) -> Result<RecorderSession> {
        self.insert_session(session, true)
    }

    fn insert_session(
        &self,
        session: NewRecorderSession,
        is_draft: bool,
    ) -> Result<RecorderSession> {
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
                    (title, created_at_ms, updated_at_ms, sample_rate, duration_ms, status, audio_path, asr_model_id, asr_engine, asr_language, is_draft)
                 VALUES (?1, ?2, ?2, ?3, 0, ?4, '', ?5, ?6, ?7, ?8)",
                params![
                    session.title,
                    session.created_at_ms,
                    session.sample_rate,
                    if is_draft { "completed" } else { "recording" },
                    session.asr_model_id,
                    session.asr_engine,
                    session.asr_language,
                    is_draft,
                ],
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

    pub fn activate_draft(
        &self,
        id: RecorderSessionId,
        sample_rate: u32,
    ) -> Result<RecorderSession> {
        if sample_rate == 0 {
            return Err(AppError::Store(
                "Recorder sample rate must be greater than zero".into(),
            ));
        }
        let changed = self
            .connection()?
            .execute(
                "UPDATE recorder_sessions
                 SET status = 'recording', is_draft = 0, sample_rate = ?1,
                     duration_ms = 0, recovery_reason = NULL
                 WHERE id = ?2 AND is_draft = 1",
                params![sample_rate, id],
            )
            .map_err(store_error)?;
        require_changed(changed, "Recorder draft", id)?;
        self.get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))
    }

    /// Re-open a completed recording for copy-on-write audio continuation.
    /// Existing transcript rows remain visible until the combined quality
    /// pass atomically replaces them after Stop.
    pub fn resume_session(&self, id: RecorderSessionId) -> Result<RecorderSession> {
        let session = self
            .get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))?;
        if session.is_draft || session.status != RecorderStatus::Completed {
            return Err(AppError::Store(format!(
                "Recorder session {id} is not a completed recording"
            )));
        }
        let partial = RecorderAudioWriter::partial_path_for(&session.audio_path);
        if partial.exists() {
            return Err(AppError::Store(format!(
                "Recorder session {id} already has partial audio to recover"
            )));
        }
        let metadata = read_caf_metadata(&session.audio_path)?;
        if metadata.sample_rate != session.sample_rate {
            return Err(AppError::Store(format!(
                "Recorder audio sample rate {} does not match session sample rate {}",
                metadata.sample_rate, session.sample_rate
            )));
        }
        let changed = self
            .connection()?
            .execute(
                "UPDATE recorder_sessions
                 SET status = 'recording', recovery_reason = NULL
                 WHERE id = ?1 AND status = 'completed' AND is_draft = 0",
                params![id],
            )
            .map_err(store_error)?;
        require_changed(changed, "Recorder session", id)?;
        self.get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))
    }

    pub fn restore_completed_after_failed_resume(&self, id: RecorderSessionId) -> Result<()> {
        let session = self
            .get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))?;
        let partial = RecorderAudioWriter::partial_path_for(&session.audio_path);
        if partial.exists() {
            if let Err(error) = std::fs::remove_file(&partial) {
                let reason = format!(
                    "Recorder continuation cleanup failed; the original audio is safe: {error}"
                );
                self.set_status(id, RecorderStatus::Recoverable, Some(&reason))?;
                return Err(AppError::Io(reason));
            }
        }
        let changed = self
            .connection()?
            .execute(
                "UPDATE recorder_sessions
                 SET status = 'completed', recovery_reason = NULL
                 WHERE id = ?1 AND status = 'recording' AND is_draft = 0",
                params![id],
            )
            .map_err(store_error)?;
        require_changed(changed, "Recorder session", id)
    }

    pub fn restore_draft_after_failed_start(&self, id: RecorderSessionId) -> Result<()> {
        let session = self
            .get_session(id)?
            .ok_or_else(|| AppError::Store(format!("Recorder session {id} was not found")))?;
        let changed = self
            .connection()?
            .execute(
                "UPDATE recorder_sessions
                 SET status = 'completed', is_draft = 1, duration_ms = 0,
                     recovery_reason = NULL
                 WHERE id = ?1 AND status = 'recording'",
                params![id],
            )
            .map_err(store_error)?;
        require_changed(changed, "Recorder session", id)?;

        // Restore the durable lifecycle first. Even if filesystem cleanup is
        // blocked, the row must never remain a phantom live capture that can
        // neither be retried as a draft nor reconciled at launch.
        let partial = RecorderAudioWriter::partial_path_for(&session.audio_path);
        for path in [&session.audio_path, &partial] {
            if path.is_file() {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
    }
}
