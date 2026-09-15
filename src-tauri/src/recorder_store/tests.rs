use super::{NewRecorderSession, RecorderStatus, RecorderStore};

fn new_session() -> NewRecorderSession {
    NewRecorderSession {
        title: "Delete failure".into(),
        created_at_ms: 1,
        sample_rate: 48_000,
        asr_model_id: "whisper-large-v3-turbo".into(),
        asr_engine: "whisper".into(),
        asr_language: "auto".into(),
    }
}

#[test]
fn delete_database_failure_leaves_a_retryable_delete_failed_session() {
    let app_dir = tempfile::tempdir().expect("temporary app support directory");
    let store = RecorderStore::open_runtime(app_dir.path()).expect("open recorder store");
    let session = store
        .create_session(new_session())
        .expect("create completed test recording");
    store
        .set_status(session.id, RecorderStatus::Completed, None)
        .expect("mark recording completed");
    std::fs::write(&session.audio_path, b"test audio").expect("create audio artifact");
    store
        .connection()
        .expect("acquire test connection")
        .execute_batch(
            "CREATE TRIGGER reject_recorder_delete
         BEFORE DELETE ON recorder_sessions
         BEGIN SELECT RAISE(ABORT, 'forced delete failure'); END;",
        )
        .expect("install deterministic delete failure");

    assert!(store.delete_session(session.id).is_err());
    let persisted = store
        .get_session(session.id)
        .expect("read session after failed deletion")
        .expect("a failed deletion must keep the row retryable");
    assert_eq!(persisted.status, RecorderStatus::DeleteFailed);
}

#[test]
fn startup_removes_committed_delete_quarantine_without_a_session_row() {
    let app_dir = tempfile::tempdir().expect("temporary app support directory");
    let session = {
        let store = RecorderStore::open_runtime(app_dir.path()).expect("open recorder store");
        let session = store
            .create_session(new_session())
            .expect("create completed test recording");
        std::fs::write(&session.audio_path, b"test audio").expect("create audio artifact");
        let quarantine = super::clear_quarantine_path(&session.audio_path);
        std::fs::rename(&session.audio_path, &quarantine).expect("simulate committed deletion");
        store
            .connection()
            .expect("acquire test connection")
            .execute_batch("BEGIN IMMEDIATE;")
            .expect("begin simulated delete transaction");
        let connection = store.connection().expect("acquire test connection");
        connection
            .execute(
                "INSERT INTO recorder_cleanup_tombstones (quarantine_path) VALUES (?1)",
                rusqlite::params![quarantine.to_string_lossy()],
            )
            .expect("persist cleanup tombstone");
        connection
            .execute(
                "DELETE FROM recorder_sessions WHERE id = ?1",
                rusqlite::params![session.id],
            )
            .expect("delete session in transaction");
        connection
            .execute_batch("COMMIT;")
            .expect("simulate database commit before cleanup");
        session
    };

    let reconciled =
        RecorderStore::open(app_dir.path()).expect("reconcile committed deletion at startup");
    assert!(
        !super::clear_quarantine_path(&session.audio_path).exists(),
        "a committed deletion must not leave recorder-owned audio behind"
    );
    let tombstones: i64 = reconciled
        .connection()
        .expect("acquire reconciled connection")
        .query_row(
            "SELECT COUNT(*) FROM recorder_cleanup_tombstones",
            [],
            |row| row.get(0),
        )
        .expect("read cleanup tombstones");
    assert_eq!(tombstones, 0, "reconciliation must finish its own marker");
}

#[test]
fn startup_never_removes_unrecognised_clearing_files() {
    let app_dir = tempfile::tempdir().expect("temporary app support directory");
    let recordings = app_dir.path().join("recordings");
    std::fs::create_dir_all(&recordings).expect("create recordings directory");
    let unrelated = recordings.join("notes-from-user.caf.clearing");
    std::fs::write(&unrelated, b"not recorder-managed").expect("create unrelated file");

    RecorderStore::open(app_dir.path()).expect("open recorder store");
    assert!(
        unrelated.exists(),
        "startup must not claim arbitrary user files"
    );
}

#[test]
fn startup_never_follows_a_tombstone_outside_recordings() {
    let app_dir = tempfile::tempdir().expect("temporary app support directory");
    let unrelated = app_dir.path().join("keep-me.caf.clearing");
    std::fs::write(&unrelated, b"not recorder-managed").expect("create unrelated file");
    {
        let store = RecorderStore::open_runtime(app_dir.path()).expect("open recorder store");
        store
            .connection()
            .expect("acquire test connection")
            .execute(
                "INSERT INTO recorder_cleanup_tombstones (quarantine_path) VALUES (?1)",
                rusqlite::params![unrelated.to_string_lossy()],
            )
            .expect("insert malformed tombstone");
    }

    RecorderStore::open(app_dir.path()).expect("open recorder store");
    assert!(
        unrelated.exists(),
        "cleanup must not follow paths outside recordings"
    );
}
