use rusqlite::Connection;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::TempDir;
use whisperpilot_lib::recorder_audio::{
    export_caf_to_wav, read_caf_audio, read_caf_metadata, RecorderAudioWriter,
};
use whisperpilot_lib::recorder_store::{
    NewRecorderSession, RecorderStatus, RecorderStore, RecorderTranscriptUpdate,
};
use whisperpilot_lib::store::{NewMeeting, Store};
use whisperpilot_lib::streaming_store::{NewStreamingSession, StreamingStore};

const DATABASE_FILE_NAME: &str = "whisperpilot.sqlite3";

fn clear_quarantine_path(path: &Path) -> std::path::PathBuf {
    let mut quarantine = path.as_os_str().to_os_string();
    quarantine.push(".clearing");
    quarantine.into()
}

fn create_recorder(
    store: &RecorderStore,
    title: &str,
    created_at_ms: i64,
    sample_rate: u32,
) -> whisperpilot_lib::recorder_store::RecorderSession {
    store
        .create_session(NewRecorderSession {
            title: title.to_string(),
            created_at_ms,
            sample_rate,
            asr_model_id: "transcription".into(),
            asr_engine: "whisper".into(),
            asr_language: "auto".into(),
        })
        .unwrap()
}

fn write_partial(final_path: &Path, sample_rate: u32, samples: &[f32]) {
    let mut writer = RecorderAudioWriter::create(final_path, sample_rate).unwrap();
    writer.append_f32(samples).unwrap();
    writer.sync_checkpoint().unwrap();
}

#[test]
fn recorder_draft_is_durable_without_audio_and_activates_in_place() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let draft = store
        .create_draft(NewRecorderSession {
            title: "Draft".into(),
            created_at_ms: 10,
            sample_rate: 48_000,
            asr_model_id: "transcription".into(),
            asr_engine: "whisper".into(),
            asr_language: "auto".into(),
        })
        .unwrap();

    assert!(draft.is_draft);
    assert!(!draft.audio_path.exists());
    store
        .rename_session(draft.id, "Renamed before recording")
        .unwrap();
    drop(store);

    let reopened = RecorderStore::open(temp.path()).unwrap();
    let persisted = reopened.get_session(draft.id).unwrap().unwrap();
    assert!(persisted.is_draft);
    assert_eq!(persisted.status, RecorderStatus::Completed);
    assert_eq!(persisted.title, "Renamed before recording");
    assert!(persisted.recovery_reason.is_none());

    let active = reopened.activate_draft(draft.id, 24_000).unwrap();
    assert!(!active.is_draft);
    assert_eq!(active.status, RecorderStatus::Recording);
    assert_eq!(active.sample_rate, 24_000);

    write_partial(&active.audio_path, 24_000, &[0.1; 2_400]);
    reopened
        .restore_draft_after_failed_start(active.id)
        .unwrap();
    let restored = reopened.get_session(active.id).unwrap().unwrap();
    assert!(restored.is_draft);
    assert_eq!(restored.status, RecorderStatus::Completed);
    assert!(!restored.audio_path.exists());
    assert!(!RecorderAudioWriter::partial_path_for(&restored.audio_path).exists());
}

#[cfg(unix)]
#[test]
fn failed_draft_cleanup_still_restores_the_non_live_draft_state() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let draft = store
        .create_draft(NewRecorderSession {
            title: "Draft".into(),
            created_at_ms: 10,
            sample_rate: 48_000,
            asr_model_id: "transcription".into(),
            asr_engine: "whisper".into(),
            asr_language: "auto".into(),
        })
        .unwrap();
    let active = store.activate_draft(draft.id, 24_000).unwrap();
    write_partial(&active.audio_path, 24_000, &[0.1; 2_400]);

    let recordings_dir = active.audio_path.parent().unwrap();
    let original_permissions = fs::metadata(recordings_dir).unwrap().permissions();
    let mut read_only_permissions = original_permissions.clone();
    read_only_permissions.set_mode(original_permissions.mode() & !0o222);
    fs::set_permissions(recordings_dir, read_only_permissions).unwrap();
    let cleanup = store.restore_draft_after_failed_start(active.id);
    fs::set_permissions(recordings_dir, original_permissions).unwrap();

    assert!(cleanup.is_err());
    let restored = store.get_session(active.id).unwrap().unwrap();
    assert!(restored.is_draft);
    assert_eq!(restored.status, RecorderStatus::Completed);
}

fn table_names(app_support_dir: &Path) -> Vec<String> {
    let connection = Connection::open(app_support_dir.join(DATABASE_FILE_NAME)).unwrap();
    let mut statement = connection
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[test]
fn recorder_schema_and_segments_are_isolated_stable_and_committed_only() {
    let temp = TempDir::new().unwrap();
    let meeting_store = Store::open(temp.path()).unwrap();
    let streaming_store = StreamingStore::open(temp.path()).unwrap();
    let recorder_store = RecorderStore::open(temp.path()).unwrap();

    let meeting = meeting_store
        .create_meeting(NewMeeting {
            title: "Meeting".into(),
            source_path: None,
            source_name: None,
            created_at_ms: 10,
            duration_ms: None,
            language: "auto".into(),
            status: "draft".into(),
        })
        .unwrap();
    let streaming = streaming_store
        .create_session(NewStreamingSession {
            title: "Streaming".into(),
            created_at_ms: 20,
        })
        .unwrap();
    let recorder = create_recorder(&recorder_store, "Recorder", 30, 44_100);
    assert_eq!(recorder.asr_model_id, "transcription");
    assert_eq!(recorder.asr_engine, "whisper");
    assert_eq!(recorder.asr_language, "auto");

    assert_eq!(
        recorder_store
            .apply_transcript_update(
                recorder.id,
                RecorderTranscriptUpdate::Partial {
                    text: "unstable partial".into(),
                },
            )
            .unwrap(),
        None
    );
    assert!(recorder_store
        .list_segments(recorder.id)
        .unwrap()
        .is_empty());

    let first = recorder_store
        .apply_transcript_update(
            recorder.id,
            RecorderTranscriptUpdate::Committed {
                start_sample: 0,
                end_sample: 22_050,
                text: "Stable first phrase".into(),
                language: "en".into(),
            },
        )
        .unwrap()
        .expect("a committed update creates a durable segment");
    let second = recorder_store
        .apply_transcript_update(
            recorder.id,
            RecorderTranscriptUpdate::Committed {
                start_sample: 22_050,
                end_sample: 44_100,
                text: "Стабильная вторая фраза".into(),
                language: "ru".into(),
            },
        )
        .unwrap()
        .unwrap();
    recorder_store
        .update_segment_text(recorder.id, first.id, "Edited stable phrase")
        .unwrap();
    recorder_store
        .rename_session(recorder.id, "Renamed Recorder")
        .unwrap();

    drop(recorder_store);
    let reopened = RecorderStore::open(temp.path()).unwrap();
    let segments = reopened.list_segments(recorder.id).unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].id, first.id);
    assert_eq!(segments[1].id, second.id);
    assert_eq!(segments[0].text, "Edited stable phrase");
    assert!(segments
        .iter()
        .all(|segment| segment.text != "unstable partial"));
    assert_eq!(
        reopened.get_session(recorder.id).unwrap().unwrap().title,
        "Renamed Recorder"
    );

    let names = table_names(temp.path());
    assert!(names.contains(&"meetings".to_string()));
    assert!(names.contains(&"streaming_sessions".to_string()));
    assert!(names.contains(&"recorder_sessions".to_string()));
    assert!(names.contains(&"recorder_segments".to_string()));
    assert!(names.contains(&"recorder_polished".to_string()));

    reopened
        .mark_recoverable(recorder.id, "test cleanup")
        .unwrap();
    reopened.delete_session(recorder.id).unwrap();
    assert!(reopened.get_session(recorder.id).unwrap().is_none());
    assert!(reopened.list_segments(recorder.id).unwrap().is_empty());
    assert!(meeting_store.get_meeting(meeting.id).unwrap().is_some());
    assert!(streaming_store.get_session(streaming.id).unwrap().is_some());
}

#[test]
fn legacy_recorder_rows_migrate_to_deterministic_whisper_identity() {
    let temp = TempDir::new().unwrap();
    let connection = Connection::open(temp.path().join(DATABASE_FILE_NAME)).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE recorder_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                sample_rate INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                audio_path TEXT NOT NULL,
                recovery_reason TEXT
            );
            INSERT INTO recorder_sessions
                (title, created_at_ms, updated_at_ms, sample_rate, duration_ms, status, audio_path)
            VALUES ('Legacy', 1, 1, 48000, 0, 'recoverable', '/missing.caf');",
        )
        .unwrap();
    drop(connection);

    let store = RecorderStore::open(temp.path()).unwrap();
    let session = store.get_session(1).unwrap().unwrap();
    assert_eq!(session.asr_model_id, "transcription");
    assert_eq!(session.asr_engine, "whisper");
    assert_eq!(session.asr_language, "auto");
    assert!(!session.is_draft);
}

#[test]
fn recorder_polish_round_trips_reverts_and_cascades_without_mutating_segments() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Polish", 50, 48_000);
    store
        .apply_transcript_update(
            recorder.id,
            RecorderTranscriptUpdate::Committed {
                start_sample: 0,
                end_sample: 48_000,
                text: "ну отправь это Alex в 15:30".into(),
                language: "ru".into(),
            },
        )
        .unwrap();

    store
        .upsert_polished(recorder.id, "Отправь это Alex в 15:30.")
        .unwrap();
    assert_eq!(
        store.get_polished(recorder.id).unwrap().as_deref(),
        Some("Отправь это Alex в 15:30.")
    );
    assert_eq!(
        store.list_segments(recorder.id).unwrap()[0].text,
        "ну отправь это Alex в 15:30"
    );

    store.delete_polished(recorder.id).unwrap();
    assert_eq!(store.get_polished(recorder.id).unwrap(), None);
    store.upsert_polished(recorder.id, "Accepted").unwrap();
    store.mark_recoverable(recorder.id, "test cleanup").unwrap();
    store.delete_session(recorder.id).unwrap();
    assert_eq!(store.get_polished(recorder.id).unwrap(), None);
}

#[test]
fn native_rate_pcm16_caf_checkpoints_finalizes_and_exports_wav() {
    let temp = TempDir::new().unwrap();
    let final_path = temp.path().join("Recorder").join("native-note.caf");
    let partial_path = RecorderAudioWriter::partial_path_for(&final_path);
    let sample_rate = 44_100;
    let mut samples = vec![0.0; sample_rate as usize + 17];
    samples[..7].copy_from_slice(&[-1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5]);

    let mut writer = RecorderAudioWriter::create(&final_path, sample_rate).unwrap();
    writer
        .append_f32(&samples[..sample_rate as usize - 1])
        .unwrap();
    assert_eq!(writer.unsynced_samples(), u64::from(sample_rate) - 1);
    writer
        .append_f32(&samples[sample_rate as usize - 1..])
        .unwrap();
    assert!(partial_path.exists());
    assert!(!final_path.exists());
    assert_eq!(writer.checkpoint_count(), 2);
    assert_eq!(writer.unsynced_samples(), 17);

    let finalized = writer.finalize().unwrap();
    assert_eq!(finalized, final_path);
    assert!(!partial_path.exists());
    assert!(final_path.exists());

    let metadata = read_caf_metadata(&final_path).unwrap();
    assert_eq!(metadata.frames, samples.len() as u64);

    let audio = read_caf_audio(&final_path).unwrap();
    assert_eq!(audio.metadata.sample_rate, sample_rate);
    assert_eq!(audio.metadata.channels, 1);
    assert_eq!(audio.metadata.bits_per_sample, 16);
    assert_eq!(audio.metadata.frames, samples.len() as u64);
    assert_eq!(
        &audio.samples[..7],
        &[i16::MIN, i16::MIN, -16_384, 0, 16_384, i16::MAX, i16::MAX]
    );

    let wav_path = temp.path().join("exported-note.wav");
    export_caf_to_wav(&final_path, &wav_path).unwrap();
    let mut wav = hound::WavReader::open(&wav_path).unwrap();
    assert_eq!(wav.spec().sample_rate, sample_rate);
    assert_eq!(wav.spec().channels, 1);
    assert_eq!(wav.spec().bits_per_sample, 16);
    assert_eq!(wav.duration(), samples.len() as u32);
    assert_eq!(
        wav.samples::<i16>()
            .take(7)
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        vec![i16::MIN, i16::MIN, -16_384, 0, 16_384, i16::MAX, i16::MAX]
    );
}

#[test]
fn continued_audio_drives_duration_and_quality_reads_from_the_combined_timeline() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Continued note", 10, 48_000);
    let mut initial = RecorderAudioWriter::create(&recorder.audio_path, 48_000).unwrap();
    initial.append_f32(&[0.25; 48_000]).unwrap();
    store
        .apply_transcript_update(
            recorder.id,
            RecorderTranscriptUpdate::Committed {
                start_sample: 0,
                end_sample: 48_000,
                text: "Existing phrase".into(),
                language: "en".into(),
            },
        )
        .unwrap();
    store.mark_finalizing(recorder.id).unwrap();
    initial.finalize().unwrap();
    store.mark_completed(recorder.id).unwrap();
    store
        .upsert_polished(recorder.id, "Existing polished phrase")
        .unwrap();

    let active = store.resume_session(recorder.id).unwrap();
    assert_eq!(active.status, RecorderStatus::Recording);
    assert_eq!(active.duration_ms, 1_000);
    assert_eq!(store.list_segments(recorder.id).unwrap().len(), 1);
    assert_eq!(
        store.get_polished(recorder.id).unwrap().as_deref(),
        Some("Existing polished phrase")
    );
    let mut resumed = RecorderAudioWriter::resume(&recorder.audio_path, 48_000).unwrap();
    resumed.append_f32(&[-0.25; 24_000]).unwrap();
    resumed.finalize().unwrap();
    store.mark_finalizing(recorder.id).unwrap();
    let completed = store.mark_completed(recorder.id).unwrap();

    let quality_input = read_caf_audio(&recorder.audio_path).unwrap();
    assert_eq!(quality_input.metadata.frames, 72_000);
    assert_eq!(&quality_input.samples[..48_000], &[8_192; 48_000]);
    assert_eq!(&quality_input.samples[48_000..], &[-8_192; 24_000]);
    assert_eq!(completed.duration_ms, 1_500);
}

#[test]
fn interrupted_continuation_recovers_the_combined_partial_over_the_old_final() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Interrupted continuation", 10, 48_000);
    let mut initial = RecorderAudioWriter::create(&recorder.audio_path, 48_000).unwrap();
    initial.append_f32(&[0.25; 48_000]).unwrap();
    store.mark_finalizing(recorder.id).unwrap();
    initial.finalize().unwrap();
    store.mark_completed(recorder.id).unwrap();

    store.resume_session(recorder.id).unwrap();
    let mut resumed = RecorderAudioWriter::resume(&recorder.audio_path, 48_000).unwrap();
    resumed.append_f32(&[-0.25; 24_000]).unwrap();
    resumed.sync_checkpoint().unwrap();
    drop(resumed);
    drop(store);

    let reopened = RecorderStore::open(temp.path()).unwrap();
    let recoverable = reopened.get_session(recorder.id).unwrap().unwrap();
    assert_eq!(recoverable.status, RecorderStatus::Recoverable);
    let recovered = reopened.recover_session(recorder.id).unwrap();
    assert_eq!(recovered.status, RecorderStatus::Completed);
    assert_eq!(recovered.duration_ms, 1_500);
    let combined = read_caf_audio(&recorder.audio_path).unwrap();
    assert_eq!(combined.metadata.frames, 72_000);
    assert_eq!(&combined.samples[..48_000], &[8_192; 48_000]);
    assert_eq!(&combined.samples[48_000..], &[-8_192; 24_000]);
}

#[test]
fn failed_resume_cleanup_keeps_the_row_recoverable_and_the_old_final_safe() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Failed continuation", 10, 48_000);
    let mut initial = RecorderAudioWriter::create(&recorder.audio_path, 48_000).unwrap();
    initial.append_f32(&[0.25; 4_800]).unwrap();
    store.mark_finalizing(recorder.id).unwrap();
    initial.finalize().unwrap();
    store.mark_completed(recorder.id).unwrap();
    let original = fs::read(&recorder.audio_path).unwrap();

    store.resume_session(recorder.id).unwrap();
    let partial = RecorderAudioWriter::partial_path_for(&recorder.audio_path);
    fs::create_dir(&partial).unwrap();
    assert!(store
        .restore_completed_after_failed_resume(recorder.id)
        .is_err());

    let recoverable = store.get_session(recorder.id).unwrap().unwrap();
    assert_eq!(recoverable.status, RecorderStatus::Recoverable);
    assert!(recoverable
        .recovery_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("cleanup failed")));
    assert_eq!(fs::read(&recorder.audio_path).unwrap(), original);
}

#[test]
fn database_completion_is_rejected_until_audio_is_closed_and_renamed() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Finalize", 100, 48_000);
    let mut writer = RecorderAudioWriter::create(&recorder.audio_path, 48_000).unwrap();
    writer.append_f32(&vec![0.25; 4_800]).unwrap();

    store.mark_finalizing(recorder.id).unwrap();
    assert!(store.mark_completed(recorder.id).is_err());
    let tail = store
        .apply_transcript_update(
            recorder.id,
            RecorderTranscriptUpdate::Committed {
                start_sample: 0,
                end_sample: 4_800,
                text: "durable tail".into(),
                language: "en".into(),
            },
        )
        .unwrap()
        .unwrap();

    writer.finalize().unwrap();
    let completed = store.mark_completed(recorder.id).unwrap();
    assert_eq!(completed.status, RecorderStatus::Completed);

    drop(store);
    let reopened = RecorderStore::open(temp.path()).unwrap();
    assert_eq!(
        reopened.get_session(recorder.id).unwrap().unwrap().status,
        RecorderStatus::Completed
    );
    assert_eq!(reopened.list_segments(recorder.id).unwrap()[0].id, tail.id);
    assert_eq!(
        read_caf_audio(&recorder.audio_path)
            .unwrap()
            .metadata
            .sample_rate,
        48_000
    );
}

#[test]
fn launch_reconciliation_preserves_every_durable_audio_database_mismatch() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let samples = vec![0.125; 1_600];

    let interrupted = create_recorder(&store, "Interrupted", 1, 16_000);
    write_partial(&interrupted.audio_path, 16_000, &samples);

    let no_artifact = create_recorder(&store, "No artifact", 0, 16_000);

    let finalizing_partial = create_recorder(&store, "Finalizing partial", 2, 16_000);
    write_partial(&finalizing_partial.audio_path, 16_000, &samples);
    store.mark_finalizing(finalizing_partial.id).unwrap();

    let finalizing_final = create_recorder(&store, "Finalizing final", 3, 16_000);
    let mut writer = RecorderAudioWriter::create(&finalizing_final.audio_path, 16_000).unwrap();
    writer.append_f32(&samples).unwrap();
    store.mark_finalizing(finalizing_final.id).unwrap();
    writer.finalize().unwrap();

    let completed_partial = create_recorder(&store, "Completed partial", 4, 16_000);
    write_partial(&completed_partial.audio_path, 16_000, &samples);
    Connection::open(temp.path().join(DATABASE_FILE_NAME))
        .unwrap()
        .execute(
            "UPDATE recorder_sessions SET status = 'completed' WHERE id = ?1",
            [completed_partial.id],
        )
        .unwrap();

    let completed_final = create_recorder(&store, "Completed final", 5, 16_000);
    let mut writer = RecorderAudioWriter::create(&completed_final.audio_path, 16_000).unwrap();
    writer.append_f32(&samples).unwrap();
    store.mark_finalizing(completed_final.id).unwrap();
    writer.finalize().unwrap();
    store.mark_completed(completed_final.id).unwrap();

    drop(store);
    let reconciled = RecorderStore::open(temp.path()).unwrap();
    for id in [
        no_artifact.id,
        interrupted.id,
        finalizing_partial.id,
        finalizing_final.id,
        completed_partial.id,
    ] {
        let session = reconciled.get_session(id).unwrap().unwrap();
        assert_eq!(session.status, RecorderStatus::Recoverable);
        assert!(session.recovery_reason.is_some());
        if id != no_artifact.id {
            assert!(
                session.audio_path.exists()
                    || RecorderAudioWriter::partial_path_for(&session.audio_path).exists()
            );
        }
    }
    assert_eq!(
        reconciled
            .get_session(completed_final.id)
            .unwrap()
            .unwrap()
            .status,
        RecorderStatus::Completed
    );
}

#[test]
fn deletion_removes_only_owned_audio_and_keeps_failed_cleanup_retryable() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Delete", 10, 24_000);
    let mut writer = RecorderAudioWriter::create(&recorder.audio_path, 24_000).unwrap();
    writer.append_f32(&vec![0.0; 2_400]).unwrap();
    store.mark_finalizing(recorder.id).unwrap();
    writer.finalize().unwrap();
    store.mark_completed(recorder.id).unwrap();

    let exported = temp.path().join("user-chosen-export.wav");
    export_caf_to_wav(&recorder.audio_path, &exported).unwrap();
    store.delete_session(recorder.id).unwrap();
    assert!(!recorder.audio_path.exists());
    assert!(exported.exists());
    assert!(store.get_session(recorder.id).unwrap().is_none());

    let failed = create_recorder(&store, "Delete failure", 20, 16_000);
    store.mark_recoverable(failed.id, "test cleanup").unwrap();
    fs::create_dir_all(&failed.audio_path).unwrap();
    assert!(store.delete_session(failed.id).is_err());
    assert_eq!(
        store.get_session(failed.id).unwrap().unwrap().status,
        RecorderStatus::DeleteFailed
    );

    fs::remove_dir(&failed.audio_path).unwrap();
    store.delete_session(failed.id).unwrap();
    assert!(store.get_session(failed.id).unwrap().is_none());
}

#[test]
fn active_sessions_cannot_be_deleted_or_recovered() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Active", 10, 16_000);
    write_partial(&recorder.audio_path, 16_000, &[0.0; 1_600]);

    assert!(store.delete_session(recorder.id).is_err());
    assert!(store.recover_session(recorder.id).is_err());
    store.mark_finalizing(recorder.id).unwrap();
    assert!(store.delete_session(recorder.id).is_err());
    assert!(store.recover_session(recorder.id).is_err());
}

#[test]
fn recovery_rejects_malformed_and_conflicting_audio_artifacts() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();

    let malformed = create_recorder(&store, "Malformed", 10, 16_000);
    fs::write(
        RecorderAudioWriter::partial_path_for(&malformed.audio_path),
        b"caff-not-a-valid-recorder-file",
    )
    .unwrap();
    store.mark_recoverable(malformed.id, "interrupted").unwrap();
    assert!(store.recover_session(malformed.id).is_err());
    assert_eq!(
        store.get_session(malformed.id).unwrap().unwrap().status,
        RecorderStatus::Recoverable
    );

    let conflict = create_recorder(&store, "Conflict", 20, 16_000);
    let mut writer = RecorderAudioWriter::create(&conflict.audio_path, 16_000).unwrap();
    writer.append_f32(&[0.0; 1_600]).unwrap();
    writer.finalize().unwrap();
    fs::copy(
        &conflict.audio_path,
        RecorderAudioWriter::partial_path_for(&conflict.audio_path),
    )
    .unwrap();
    store
        .mark_recoverable(conflict.id, "two artifacts")
        .unwrap();
    assert!(store.recover_session(conflict.id).is_err());
    assert!(RecorderAudioWriter::partial_path_for(&conflict.audio_path).exists());
}

#[test]
fn completed_duration_comes_from_native_audio_frames_even_without_transcript() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Silent", 10, 48_000);
    let mut writer = RecorderAudioWriter::create(&recorder.audio_path, 48_000).unwrap();
    writer.append_f32(&vec![0.0; 72_000]).unwrap();
    store.mark_finalizing(recorder.id).unwrap();
    writer.finalize().unwrap();

    let completed = store.mark_completed(recorder.id).unwrap();
    assert_eq!(completed.duration_ms, 1_500);
}

#[test]
fn clearing_a_recorder_removes_audio_and_text_but_retains_an_empty_draft() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Keep audio", 10, 48_000);
    write_partial(&recorder.audio_path, 48_000, &[0.0; 4_800]);
    let audio_path = RecorderAudioWriter::partial_path_for(&recorder.audio_path);
    store
        .apply_transcript_update(
            recorder.id,
            RecorderTranscriptUpdate::Committed {
                start_sample: 0,
                end_sample: 4_800,
                text: "raw transcript".into(),
                language: "en".into(),
            },
        )
        .unwrap();
    store
        .upsert_polished(recorder.id, "prettified transcript")
        .unwrap();
    store.mark_recoverable(recorder.id, "test fixture").unwrap();

    store.clear_recording(recorder.id).unwrap();

    let cleared = store.get_session(recorder.id).unwrap().unwrap();
    assert!(cleared.is_draft);
    assert_eq!(cleared.status, RecorderStatus::Completed);
    assert_eq!(cleared.duration_ms, 0);
    assert_eq!(cleared.recovery_reason, None);
    assert!(store.list_segments(recorder.id).unwrap().is_empty());
    assert_eq!(store.get_polished(recorder.id).unwrap(), None);
    assert!(!audio_path.exists());
}

#[test]
fn a_late_polish_accept_cannot_restore_content_after_clear() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Clear race", 10, 48_000);
    store
        .apply_transcript_update(
            recorder.id,
            RecorderTranscriptUpdate::Committed {
                start_sample: 0,
                end_sample: 48_000,
                text: "original".into(),
                language: "en".into(),
            },
        )
        .unwrap();
    store.mark_recoverable(recorder.id, "test fixture").unwrap();
    store.clear_recording(recorder.id).unwrap();

    assert!(store.upsert_polished(recorder.id, "late polish").is_err());
    assert_eq!(store.get_polished(recorder.id).unwrap(), None);
}

#[test]
fn clearing_a_live_recorder_is_rejected_without_data_loss() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Live", 10, 48_000);
    store
        .apply_transcript_update(
            recorder.id,
            RecorderTranscriptUpdate::Committed {
                start_sample: 0,
                end_sample: 4_800,
                text: "still recording".into(),
                language: "en".into(),
            },
        )
        .unwrap();

    assert!(store.clear_recording(recorder.id).is_err());
    assert_eq!(store.list_segments(recorder.id).unwrap().len(), 1);
}

#[test]
fn failed_audio_cleanup_keeps_the_recorder_text_and_non_draft_state() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Blocked clear", 10, 48_000);
    store
        .apply_transcript_update(
            recorder.id,
            RecorderTranscriptUpdate::Committed {
                start_sample: 0,
                end_sample: 4_800,
                text: "must survive".into(),
                language: "en".into(),
            },
        )
        .unwrap();
    store
        .upsert_polished(recorder.id, "must survive too")
        .unwrap();
    store.mark_recoverable(recorder.id, "test fixture").unwrap();
    fs::create_dir_all(&recorder.audio_path).unwrap();

    assert!(store.clear_recording(recorder.id).is_err());

    let unchanged = store.get_session(recorder.id).unwrap().unwrap();
    assert!(!unchanged.is_draft);
    assert_eq!(unchanged.status, RecorderStatus::Recoverable);
    assert_eq!(store.list_segments(recorder.id).unwrap().len(), 1);
    assert_eq!(
        store.get_polished(recorder.id).unwrap().as_deref(),
        Some("must survive too")
    );
}

#[test]
fn failed_clear_database_commit_restores_quarantined_audio_and_text() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Rollback clear", 10, 48_000);
    store
        .apply_transcript_update(
            recorder.id,
            RecorderTranscriptUpdate::Committed {
                start_sample: 0,
                end_sample: 4_800,
                text: "must survive rollback".into(),
                language: "en".into(),
            },
        )
        .unwrap();
    store.mark_finalizing(recorder.id).unwrap();
    let mut writer = RecorderAudioWriter::create(&recorder.audio_path, 48_000).unwrap();
    writer.append_f32(&[0.1; 4_800]).unwrap();
    writer.finalize().unwrap();
    store.mark_completed(recorder.id).unwrap();
    let connection = Connection::open(temp.path().join(DATABASE_FILE_NAME)).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_recorder_clear
             BEFORE UPDATE OF is_draft ON recorder_sessions
             BEGIN
                 SELECT RAISE(ABORT, 'injected clear failure');
             END;",
        )
        .unwrap();
    drop(connection);

    assert!(store.clear_recording(recorder.id).is_err());

    let unchanged = store.get_session(recorder.id).unwrap().unwrap();
    assert!(!unchanged.is_draft);
    assert_eq!(unchanged.status, RecorderStatus::Completed);
    assert_eq!(store.list_segments(recorder.id).unwrap().len(), 1);
    assert!(recorder.audio_path.is_file());
}

#[test]
fn launch_reconciliation_restores_a_precommit_clear_quarantine() {
    let temp = TempDir::new().unwrap();
    let recorder = {
        let store = RecorderStore::open(temp.path()).unwrap();
        let recorder = create_recorder(&store, "Interrupted clear", 10, 48_000);
        store
            .apply_transcript_update(
                recorder.id,
                RecorderTranscriptUpdate::Committed {
                    start_sample: 0,
                    end_sample: 4_800,
                    text: "must survive restart".into(),
                    language: "en".into(),
                },
            )
            .unwrap();
        store.mark_finalizing(recorder.id).unwrap();
        let mut writer = RecorderAudioWriter::create(&recorder.audio_path, 48_000).unwrap();
        writer.append_f32(&[0.1; 4_800]).unwrap();
        writer.finalize().unwrap();
        store.mark_completed(recorder.id).unwrap();
        recorder
    };
    let quarantine = clear_quarantine_path(&recorder.audio_path);
    fs::rename(&recorder.audio_path, &quarantine).unwrap();

    let reopened = RecorderStore::open(temp.path()).unwrap();

    assert!(recorder.audio_path.is_file());
    assert!(!quarantine.exists());
    assert_eq!(
        reopened.get_session(recorder.id).unwrap().unwrap().status,
        RecorderStatus::Completed
    );
    assert_eq!(reopened.list_segments(recorder.id).unwrap().len(), 1);
}

#[test]
fn launch_reconciliation_removes_a_postcommit_clear_quarantine() {
    let temp = TempDir::new().unwrap();
    let recorder = {
        let store = RecorderStore::open(temp.path()).unwrap();
        let recorder = create_recorder(&store, "Committed clear", 10, 48_000);
        store.mark_finalizing(recorder.id).unwrap();
        let mut writer = RecorderAudioWriter::create(&recorder.audio_path, 48_000).unwrap();
        writer.append_f32(&[0.1; 4_800]).unwrap();
        writer.finalize().unwrap();
        store.mark_completed(recorder.id).unwrap();
        recorder
    };
    let quarantine = clear_quarantine_path(&recorder.audio_path);
    fs::rename(&recorder.audio_path, &quarantine).unwrap();
    let connection = Connection::open(temp.path().join(DATABASE_FILE_NAME)).unwrap();
    connection
        .execute(
            "UPDATE recorder_sessions
             SET status = 'completed', is_draft = 1, duration_ms = 0
             WHERE id = ?1",
            [recorder.id],
        )
        .unwrap();
    drop(connection);

    let reopened = RecorderStore::open(temp.path()).unwrap();

    assert!(!quarantine.exists());
    assert!(!recorder.audio_path.exists());
    assert!(reopened.get_session(recorder.id).unwrap().unwrap().is_draft);
}

#[test]
fn quality_pass_replaces_live_segments_atomically_and_clears_stale_polish() {
    let temp = TempDir::new().unwrap();
    let store = RecorderStore::open(temp.path()).unwrap();
    let recorder = create_recorder(&store, "Refine", 10, 48_000);
    store
        .apply_transcript_update(
            recorder.id,
            RecorderTranscriptUpdate::Committed {
                start_sample: 0,
                end_sample: 48_000,
                text: "rough live text".into(),
                language: "en".into(),
            },
        )
        .unwrap();
    store
        .upsert_polished(recorder.id, "stale polished text")
        .unwrap();
    store.mark_finalizing(recorder.id).unwrap();

    store
        .replace_transcript(
            recorder.id,
            vec![
                RecorderTranscriptUpdate::Committed {
                    start_sample: 0,
                    end_sample: 24_000,
                    text: "Accurate first phrase".into(),
                    language: "en".into(),
                },
                RecorderTranscriptUpdate::Committed {
                    start_sample: 24_000,
                    end_sample: 48_000,
                    text: "Точная вторая фраза".into(),
                    language: "ru".into(),
                },
            ],
        )
        .unwrap();

    let segments = store.list_segments(recorder.id).unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].text, "Accurate first phrase");
    assert_eq!(segments[1].text, "Точная вторая фраза");
    assert_eq!(store.get_polished(recorder.id).unwrap(), None);
}
