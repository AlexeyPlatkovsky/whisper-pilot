use super::*;
use crate::error::AppError;

fn draft(title: &str, created_at_ms: i64) -> NewStreamingSession {
    NewStreamingSession {
        title: title.to_string(),
        created_at_ms,
    }
}

fn window(window_index: i64, start_ms: i64, end_ms: i64) -> NewStreamingWindow {
    NewStreamingWindow {
        window_index,
        start_ms,
        end_ms,
        text: format!("window {window_index}"),
        language: "en".to_string(),
        outcome_ok: true,
    }
}

fn seed_transcript(store: &StreamingStore, session_id: StreamingSessionId) {
    store
        .append_window(session_id, &window(0, 0, 1_000), 1_100)
        .expect("seed transcript");
}

#[test]
fn opening_the_store_configures_wal_and_a_bounded_busy_wait() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let connection = store.connection().expect("lock database connection");

    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("read journal mode");
    let busy_timeout_ms: u64 = connection
        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
        .expect("read busy timeout");

    assert_eq!(journal_mode, "wal");
    assert_eq!(busy_timeout_ms, SQLITE_BUSY_TIMEOUT.as_millis() as u64);
}

#[test]
fn given_empty_directory_when_creating_session_then_it_persists_and_lists() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");

    let created = store
        .create_session(draft("Standup", 100))
        .expect("create session");

    assert_eq!(created.status, status::STOPPED);
    // WP-101: a newly created session defaults translation_enabled to
    // false — nothing has been persisted for it yet.
    assert!(!created.translation_enabled);
    assert_eq!(
        store.get_session(created.id).expect("get session"),
        Some(created.clone())
    );
    assert_eq!(
        store.list_sessions().expect("list sessions"),
        vec![StreamingSessionSummary {
            id: created.id,
            title: "Standup".to_string(),
            created_at_ms: 100,
            updated_at_ms: 100,
            duration_ms: 0,
            status: status::STOPPED.to_string(),
            translation_enabled: false,
            translation_target_language: "ru".to_string(),
        }]
    );
}

#[test]
fn persists_a_non_secret_cloud_configuration_once_before_capture() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session = store
        .create_session(draft("Cloud standup", 100))
        .expect("create session");
    let config = StreamingSessionConfiguration {
        engine: "cloud".to_string(),
        cloud_provider: Some("openai".to_string()),
        cloud_model: Some("gpt-transcribe".to_string()),
    };

    store
        .set_session_configuration(session.id, &config)
        .expect("store configuration");
    assert_eq!(
        store
            .get_session_configuration(session.id)
            .expect("read configuration"),
        Some(config)
    );
}

#[test]
fn rejects_configuration_changes_after_a_session_becomes_active() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session = store
        .create_session(draft("Active stream", 100))
        .expect("create session");
    store.mark_active(session.id, 200).expect("start session");

    let error = store
        .set_session_configuration(
            session.id,
            &StreamingSessionConfiguration {
                engine: "local".to_string(),
                cloud_provider: None,
                cloud_model: None,
            },
        )
        .expect_err("active session cannot change engine");
    assert!(error.to_string().contains("cannot change"));
}

#[test]
fn given_saved_windows_when_reopened_then_session_and_windows_persist_in_order() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let session_id;

    {
        let store = StreamingStore::open(temp.path()).expect("open database");
        session_id = store
            .create_session(draft("Live thoughts", 100))
            .expect("create session")
            .id;
        store
            .append_window(session_id, &window(1, 7_000, 14_000), 14_500)
            .expect("append window 1");
        store
            .append_window(session_id, &window(0, 0, 7_000), 7_500)
            .expect("append window 0");
    }

    let reopened = StreamingStore::open(temp.path()).expect("reopen database");
    assert_eq!(
        reopened
            .list_windows(session_id)
            .expect("list windows")
            .into_iter()
            .map(|w| w.window_index)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    // The session's updated_at_ms reflects the last append, not creation.
    assert_eq!(
        reopened
            .get_session(session_id)
            .expect("get session")
            .map(|s| s.updated_at_ms),
        Some(7_500)
    );
}

#[test]
fn recovers_mixed_streaming_mfu_schema_with_orphaned_legacy_rows() {
    // Decision table: streaming_notes-only migrates; mixed schemas preserve current rows, import valid legacy rows, and discard orphans.
    let legacy_only = tempfile::tempdir().expect("legacy-only app support");
    let connection = Connection::open(crate::store::shared_database_path(legacy_only.path()))
        .expect("open legacy db");
    connection
            .execute_batch(
                "CREATE TABLE streaming_sessions (id INTEGER PRIMARY KEY, title TEXT NOT NULL, created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL, status TEXT NOT NULL);
                 CREATE TABLE streaming_notes (session_id INTEGER PRIMARY KEY, summary TEXT NOT NULL, decisions TEXT NOT NULL, action_items TEXT NOT NULL, open_questions TEXT NOT NULL, participants TEXT NOT NULL);
                 INSERT INTO streaming_sessions VALUES (1, 'Legacy', 1, 1, 'stopped');
                 INSERT INTO streaming_notes VALUES (1, 'Legacy summary', 'Legacy decisions', 'Legacy actions', 'Legacy questions', 'Legacy participants');",
            )
            .expect("seed legacy db");
    drop(connection);

    let legacy_store = StreamingStore::open(legacy_only.path()).expect("migrate legacy db");
    assert_eq!(
        legacy_store.get_mfu(1).expect("read migrated mfu"),
        Some(StreamingMfu {
            session_id: 1,
            summary: "Legacy summary".to_string(),
            decisions: "Legacy decisions".to_string(),
            action_items: "Legacy actions".to_string(),
            open_questions: "Legacy questions".to_string(),
            participants: "Legacy participants".to_string(),
        })
    );
    drop(legacy_store);
    let reopened_legacy =
        StreamingStore::open(legacy_only.path()).expect("repeat legacy migration");
    assert_eq!(
        reopened_legacy
            .get_mfu(1)
            .expect("read reopened legacy mfu"),
        Some(StreamingMfu {
            session_id: 1,
            summary: "Legacy summary".to_string(),
            decisions: "Legacy decisions".to_string(),
            action_items: "Legacy actions".to_string(),
            open_questions: "Legacy questions".to_string(),
            participants: "Legacy participants".to_string(),
        })
    );
    drop(reopened_legacy);
    let legacy_notes_exist = Connection::open(crate::store::shared_database_path(legacy_only.path()))
            .expect("reopen legacy db")
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'streaming_notes')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .expect("read legacy schema");
    assert!(!legacy_notes_exist);

    let mixed = tempfile::tempdir().expect("mixed-schema app support");
    let connection =
        Connection::open(crate::store::shared_database_path(mixed.path())).expect("open mixed db");
    connection
            .execute_batch(
                "CREATE TABLE streaming_sessions (id INTEGER PRIMARY KEY, title TEXT NOT NULL, created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL, status TEXT NOT NULL);
                 CREATE TABLE streaming_notes (session_id INTEGER PRIMARY KEY, summary TEXT NOT NULL, decisions TEXT NOT NULL, action_items TEXT NOT NULL, open_questions TEXT NOT NULL, participants TEXT NOT NULL);
                 CREATE TABLE streaming_mfu (session_id INTEGER PRIMARY KEY REFERENCES streaming_sessions(id) ON DELETE CASCADE, summary TEXT NOT NULL, decisions TEXT NOT NULL, action_items TEXT NOT NULL, open_questions TEXT NOT NULL, participants TEXT NOT NULL);
                 INSERT INTO streaming_sessions VALUES (1, 'Current', 1, 1, 'stopped'), (2, 'Legacy', 2, 2, 'stopped');
                 INSERT INTO streaming_mfu VALUES (1, 'Current summary', 'Current decisions', 'Current actions', 'Current questions', 'Current participants');
                 INSERT INTO streaming_notes VALUES (1, 'Stale summary', 'Stale decisions', 'Stale actions', 'Stale questions', 'Stale participants'), (2, 'Imported summary', 'Imported decisions', 'Imported actions', 'Imported questions', 'Imported participants'), (3, 'Orphaned summary', 'Orphaned decisions', 'Orphaned actions', 'Orphaned questions', 'Orphaned participants');",
            )
            .expect("seed mixed db");
    drop(connection);

    let mixed_store = StreamingStore::open(mixed.path()).expect("recover mixed db");
    assert_eq!(
        mixed_store.get_mfu(1).expect("read current mfu"),
        Some(StreamingMfu {
            session_id: 1,
            summary: "Current summary".to_string(),
            decisions: "Current decisions".to_string(),
            action_items: "Current actions".to_string(),
            open_questions: "Current questions".to_string(),
            participants: "Current participants".to_string(),
        })
    );
    assert_eq!(
        mixed_store.get_mfu(2).expect("read imported mfu"),
        Some(StreamingMfu {
            session_id: 2,
            summary: "Imported summary".to_string(),
            decisions: "Imported decisions".to_string(),
            action_items: "Imported actions".to_string(),
            open_questions: "Imported questions".to_string(),
            participants: "Imported participants".to_string(),
        })
    );
    assert_eq!(mixed_store.get_mfu(3).expect("read orphaned mfu"), None);
    drop(mixed_store);
    let reopened_mixed = StreamingStore::open(mixed.path()).expect("repeat mixed migration");
    assert_eq!(
        reopened_mixed
            .get_mfu(1)
            .expect("read reopened current mfu"),
        Some(StreamingMfu {
            session_id: 1,
            summary: "Current summary".to_string(),
            decisions: "Current decisions".to_string(),
            action_items: "Current actions".to_string(),
            open_questions: "Current questions".to_string(),
            participants: "Current participants".to_string(),
        })
    );
    assert_eq!(
        reopened_mixed
            .get_mfu(3)
            .expect("read reopened orphaned mfu"),
        None
    );
    assert_eq!(
        reopened_mixed
            .get_mfu(2)
            .expect("read reopened imported mfu"),
        Some(StreamingMfu {
            session_id: 2,
            summary: "Imported summary".to_string(),
            decisions: "Imported decisions".to_string(),
            action_items: "Imported actions".to_string(),
            open_questions: "Imported questions".to_string(),
            participants: "Imported participants".to_string(),
        })
    );
    drop(reopened_mixed);
    let mixed_notes_exist = Connection::open(crate::store::shared_database_path(mixed.path()))
            .expect("reopen mixed db")
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'streaming_notes')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .expect("read mixed schema");
    assert!(!mixed_notes_exist);
}

#[test]
fn appending_the_same_window_index_twice_overwrites_rather_than_duplicates() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store
        .create_session(draft("Retry", 100))
        .expect("create session")
        .id;

    store
        .append_window(session_id, &window(0, 0, 7_000), 7_100)
        .expect("first append");
    let mut retried = window(0, 0, 7_000);
    retried.text = "corrected text".to_string();
    store
        .append_window(session_id, &retried, 7_200)
        .expect("retried append (idempotent upsert)");

    let windows = store.list_windows(session_id).expect("list windows");
    assert_eq!(windows.len(), 1, "retry must overwrite, not duplicate");
    assert_eq!(windows[0].text, "corrected text");
}

#[test]
fn a_failed_window_is_stored_with_outcome_ok_false_not_indistinguishable_silence() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store
        .create_session(draft("Fail-open", 100))
        .expect("create session")
        .id;

    let failed = NewStreamingWindow {
        window_index: 0,
        start_ms: 0,
        end_ms: 7_000,
        text: String::new(),
        language: "auto".to_string(),
        outcome_ok: false,
    };
    store
        .append_window(session_id, &failed, 7_100)
        .expect("append failed window");

    let windows = store.list_windows(session_id).expect("list windows");
    assert!(!windows[0].outcome_ok);
}

#[test]
fn appending_to_an_unknown_session_is_a_store_error_not_a_silent_insert() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");

    let result = store.append_window(999_999, &window(0, 0, 7_000), 100);

    assert!(matches!(result, Err(AppError::Store(_))));
}

#[test]
fn given_session_with_windows_when_deleted_then_windows_are_cascaded() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store
        .create_session(draft("Disposable", 100))
        .expect("create session")
        .id;
    store
        .append_window(session_id, &window(0, 0, 7_000), 7_100)
        .expect("append window");

    store.delete_session(session_id).expect("delete session");

    assert_eq!(store.get_session(session_id).expect("get session"), None);
    assert!(store
        .list_windows(session_id)
        .expect("list windows")
        .is_empty());
}

#[test]
fn renaming_an_unknown_session_is_a_store_error() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");

    let result = store.rename_session(999_999, "New title");

    assert!(matches!(result, Err(AppError::Store(_))));
}

#[test]
fn marking_a_session_stopped_updates_status_and_freshness() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store
        .create_session(draft("Ends", 100))
        .expect("create session")
        .id;

    store.mark_stopped(session_id, 5_000).expect("mark stopped");

    let session = store
        .get_session(session_id)
        .expect("get session")
        .expect("session exists");
    assert_eq!(session.status, status::STOPPED);
    assert_eq!(session.updated_at_ms, 5_000);
}

#[test]
fn two_streaming_sessions_have_independent_window_sequences() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let a = store.create_session(draft("A", 100)).unwrap().id;
    let b = store.create_session(draft("B", 200)).unwrap().id;

    store.append_window(a, &window(0, 0, 7_000), 7_100).unwrap();
    store.append_window(b, &window(0, 0, 7_000), 7_100).unwrap();

    assert_eq!(store.list_windows(a).unwrap().len(), 1);
    assert_eq!(store.list_windows(b).unwrap().len(), 1);
}

fn mfu(session_id: StreamingSessionId) -> StreamingMfu {
    StreamingMfu {
        session_id,
        summary: "Discussed Q3 roadmap.".to_string(),
        decisions: "Ship M1 by Friday.".to_string(),
        action_items: "Alex: update deck".to_string(),
        open_questions: "Budget for Q4?".to_string(),
        participants: "Alex, Sam".to_string(),
    }
}

#[test]
fn given_no_mfu_when_getting_then_result_is_none() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

    assert_eq!(store.get_mfu(session_id).expect("get mfu"), None);
}

#[test]
fn marking_a_stopped_session_active_again_updates_status_and_freshness() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store
        .create_session(draft("Resumable", 100))
        .expect("create session")
        .id;
    store.mark_stopped(session_id, 5_000).expect("mark stopped");

    store.mark_active(session_id, 9_000).expect("mark active");

    let session = store
        .get_session(session_id)
        .expect("get session")
        .expect("session exists");
    assert_eq!(session.status, status::ACTIVE);
    assert_eq!(session.updated_at_ms, 9_000);
}

#[test]
fn marking_an_unknown_session_active_is_a_store_error() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");

    let result = store.mark_active(999_999, 100);

    assert!(matches!(result, Err(AppError::Store(_))));
}

#[test]
fn upserted_mfu_round_trip() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
    seed_transcript(&store, session_id);

    store.upsert_mfu(&mfu(session_id)).expect("upsert mfu");

    assert_eq!(
        store.get_mfu(session_id).expect("get mfu"),
        Some(mfu(session_id))
    );
}

#[test]
fn upserting_mfu_twice_overwrites_rather_than_duplicates() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
    seed_transcript(&store, session_id);
    store.upsert_mfu(&mfu(session_id)).expect("first upsert");

    let mut second = mfu(session_id);
    second.summary = "Revised summary.".to_string();
    store.upsert_mfu(&second).expect("second upsert");

    assert_eq!(store.get_mfu(session_id).expect("get mfu"), Some(second));
}

#[test]
fn upserting_mfu_for_a_nonexistent_session_is_a_store_error() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");

    // The streaming_mfu.session_id foreign key rejects this without
    // any application-level existence check needed.
    assert!(store.upsert_mfu(&mfu(999_999)).is_err());
}

#[test]
fn deleting_a_session_cascades_its_mfu() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
    seed_transcript(&store, session_id);
    store.upsert_mfu(&mfu(session_id)).expect("upsert mfu");

    store.delete_session(session_id).expect("delete session");

    assert_eq!(store.get_mfu(session_id).expect("get mfu"), None);
}

#[test]
fn deleting_mfu_directly_leaves_the_session_intact() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
    seed_transcript(&store, session_id);
    store.upsert_mfu(&mfu(session_id)).expect("upsert mfu");

    store.delete_mfu(session_id).expect("delete mfu");

    assert_eq!(store.get_mfu(session_id).expect("get mfu"), None);
    assert!(store
        .get_session(session_id)
        .expect("get session")
        .is_some());
}

#[test]
fn given_no_prettified_text_when_getting_then_result_is_none() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

    assert_eq!(
        store.get_prettified(session_id).expect("get prettified"),
        None
    );
}

#[test]
fn upserted_prettified_text_round_trips() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
    seed_transcript(&store, session_id);

    store
        .upsert_prettified(session_id, "Cleaned transcript text.")
        .expect("upsert prettified");

    assert_eq!(
        store.get_prettified(session_id).expect("get prettified"),
        Some("Cleaned transcript text.".to_string())
    );
}

#[test]
fn upserting_prettified_text_twice_overwrites_rather_than_duplicates() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
    seed_transcript(&store, session_id);
    store
        .upsert_prettified(session_id, "First version.")
        .expect("first upsert");

    store
        .upsert_prettified(session_id, "Revised version.")
        .expect("second upsert");

    assert_eq!(
        store.get_prettified(session_id).expect("get prettified"),
        Some("Revised version.".to_string())
    );
}

#[test]
fn upserting_prettified_text_for_a_nonexistent_session_is_a_store_error() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");

    assert!(store.upsert_prettified(999_999, "text").is_err());
}

#[test]
fn deleting_a_session_cascades_its_prettified_text() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
    seed_transcript(&store, session_id);
    store
        .upsert_prettified(session_id, "Cleaned text.")
        .expect("upsert prettified");

    store.delete_session(session_id).expect("delete session");

    assert_eq!(
        store.get_prettified(session_id).expect("get prettified"),
        None
    );
}

#[test]
fn deleting_prettified_text_directly_leaves_the_session_intact() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
    seed_transcript(&store, session_id);
    store
        .upsert_prettified(session_id, "Cleaned text.")
        .expect("upsert prettified");

    store
        .delete_prettified(session_id)
        .expect("delete prettified");

    assert_eq!(
        store.get_prettified(session_id).expect("get prettified"),
        None
    );
    assert!(store
        .get_session(session_id)
        .expect("get session")
        .is_some());
}

// --- WP-92: streaming_translations ---

fn translation(
    session_id: StreamingSessionId,
    window_index: i64,
    target_language: &str,
) -> StreamingTranslation {
    StreamingTranslation {
        session_id,
        window_index,
        target_language: target_language.to_string(),
        source_text: "Привет, мир.".to_string(),
        translated_text: "Hello, world.".to_string(),
        updated_at_ms: 1_000,
    }
}

#[test]
fn given_no_translations_when_listing_then_result_is_empty() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

    assert!(store
        .list_translations(session_id, "en")
        .expect("list translations")
        .is_empty());
}

#[test]
fn upserted_translation_round_trips_through_list() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

    store
        .upsert_translation(&translation(session_id, 0, "en"))
        .expect("upsert translation");

    assert_eq!(
        store.list_translations(session_id, "en").expect("list"),
        vec![translation(session_id, 0, "en")]
    );
}

#[test]
fn upserting_the_same_window_index_and_language_twice_overwrites_rather_than_duplicates() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
    store
        .upsert_translation(&translation(session_id, 0, "en"))
        .expect("first upsert");

    let mut revised = translation(session_id, 0, "en");
    revised.translated_text = "Hello, everyone.".to_string();
    revised.updated_at_ms = 2_000;
    store
        .upsert_translation(&revised)
        .expect("second upsert (retranslate)");

    let rows = store.list_translations(session_id, "en").expect("list");
    assert_eq!(rows.len(), 1, "retranslation must overwrite, not duplicate");
    assert_eq!(rows[0].translated_text, "Hello, everyone.");
    assert_eq!(rows[0].updated_at_ms, 2_000);
}

#[test]
fn translations_for_different_target_languages_are_independent_rows() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;

    store
        .upsert_translation(&translation(session_id, 0, "en"))
        .expect("upsert en");
    store
        .upsert_translation(&translation(session_id, 0, "ru"))
        .expect("upsert ru");

    assert_eq!(store.list_translations(session_id, "en").unwrap().len(), 1);
    assert_eq!(store.list_translations(session_id, "ru").unwrap().len(), 1);
}

#[test]
fn translations_for_different_sessions_are_independent() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let a = store.create_session(draft("A", 100)).unwrap().id;
    let b = store.create_session(draft("B", 200)).unwrap().id;

    store
        .upsert_translation(&translation(a, 0, "en"))
        .expect("upsert for a");

    assert_eq!(store.list_translations(a, "en").unwrap().len(), 1);
    assert!(store.list_translations(b, "en").unwrap().is_empty());
}

#[test]
fn upserting_a_translation_for_a_nonexistent_session_is_a_store_error() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");

    let result = store.upsert_translation(&translation(999_999, 0, "en"));

    assert!(matches!(result, Err(AppError::Store(_))));
}

#[test]
fn deleting_a_session_cascades_its_translations() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Disposable", 100)).unwrap().id;
    store
        .upsert_translation(&translation(session_id, 0, "en"))
        .expect("upsert translation");

    store.delete_session(session_id).expect("delete session");

    assert!(store
        .list_translations(session_id, "en")
        .unwrap()
        .is_empty());
}

#[test]
fn streaming_translation_reports_stale_when_source_text_no_longer_matches() {
    let stored = translation(1, 0, "en");
    assert!(stored.is_stale("Привет, мир! (изменено)"));
}

#[test]
fn streaming_translation_reports_not_stale_when_source_text_still_matches() {
    let stored = translation(1, 0, "en");
    assert!(!stored.is_stale("Привет, мир."));
}

/// Opening a database that predates the `streaming_translations` table
/// (but already has sessions/segments/mfu/prettified data) must both
/// preserve that existing data and make the new table usable —
/// `CREATE TABLE IF NOT EXISTS` migration, same shape as the
/// `streaming_prettified` precedent.
#[test]
fn opening_a_pre_migration_database_preserves_existing_data_and_adds_translations_table() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let db_path = crate::store::shared_database_path(temp.path());
    std::fs::create_dir_all(temp.path()).expect("create app-support dir");

    {
        // Build a pre-WP-92 database by hand: every table this feature's
        // migration must leave intact, deliberately excluding
        // streaming_translations.
        let connection = Connection::open(&db_path).expect("open raw pre-migration database");
        connection
                .execute_batch(
                    r#"
                    CREATE TABLE streaming_sessions (
                        id INTEGER PRIMARY KEY,
                        title TEXT NOT NULL,
                        created_at_ms INTEGER NOT NULL,
                        updated_at_ms INTEGER NOT NULL,
                        status TEXT NOT NULL
                    );
                    CREATE TABLE streaming_segments (
                        session_id INTEGER NOT NULL REFERENCES streaming_sessions(id) ON DELETE CASCADE,
                        window_index INTEGER NOT NULL,
                        start_ms INTEGER NOT NULL CHECK(start_ms >= 0),
                        end_ms INTEGER NOT NULL CHECK(end_ms >= start_ms),
                        text TEXT NOT NULL,
                        language TEXT NOT NULL,
                        outcome_ok INTEGER NOT NULL,
                        PRIMARY KEY (session_id, window_index)
                    );
                    CREATE TABLE streaming_mfu (
                        session_id INTEGER PRIMARY KEY REFERENCES streaming_sessions(id) ON DELETE CASCADE,
                        summary TEXT NOT NULL,
                        decisions TEXT NOT NULL,
                        action_items TEXT NOT NULL,
                        open_questions TEXT NOT NULL,
                        participants TEXT NOT NULL
                    );
                    CREATE TABLE streaming_prettified (
                        session_id INTEGER PRIMARY KEY REFERENCES streaming_sessions(id) ON DELETE CASCADE,
                        text TEXT NOT NULL
                    );

                    INSERT INTO streaming_sessions (id, title, created_at_ms, updated_at_ms, status)
                        VALUES (1, 'Pre-migration session', 100, 200, 'stopped');
                    INSERT INTO streaming_segments
                        (session_id, window_index, start_ms, end_ms, text, language, outcome_ok)
                        VALUES (1, 0, 0, 7000, 'hello there', 'en', 1);
                    INSERT INTO streaming_mfu
                        (session_id, summary, decisions, action_items, open_questions, participants)
                        VALUES (1, 'Summary.', 'Decisions.', 'Actions.', 'Questions.', 'Alex');
                    INSERT INTO streaming_prettified (session_id, text)
                        VALUES (1, 'Cleaned transcript.');
                    "#,
                )
                .expect("seed pre-migration schema and data");
    }

    let store = StreamingStore::open(temp.path()).expect("open (and migrate) database");

    // Pre-existing data across every prior streaming table survived.
    let session = store
        .get_session(1)
        .expect("get session")
        .expect("session survives migration");
    assert_eq!(session.title, "Pre-migration session");
    assert_eq!(session.status, status::STOPPED);

    let windows = store.list_windows(1).expect("list windows");
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].text, "hello there");

    assert_eq!(
        store.get_mfu(1).expect("get mfu").map(|m| m.summary),
        Some("Summary.".to_string())
    );
    assert_eq!(
        store.get_prettified(1).expect("get prettified"),
        Some("Cleaned transcript.".to_string())
    );

    // The new table exists and is immediately usable.
    assert!(store.list_translations(1, "en").expect("list").is_empty());
    store
        .upsert_translation(&translation(1, 0, "en"))
        .expect("upsert into migrated table");
    assert_eq!(store.list_translations(1, "en").expect("list").len(), 1);
}

// --- WP-103: paragraph_key -> window_index column rename ---

/// Opening a database whose `streaming_translations` table still has the
/// pre-WP-103 `paragraph_key` column must rename it to `window_index` in
/// place via `ALTER TABLE ... RENAME COLUMN`, preserving existing rows —
/// the same checked-before-ALTER idiom `migrate_translation_enabled_column`
/// uses, applied to a rename instead of an add.
#[test]
fn migrating_a_pre_rename_paragraph_key_column_renames_it_to_window_index_and_preserves_data() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let db_path = crate::store::shared_database_path(temp.path());
    std::fs::create_dir_all(temp.path()).expect("create app-support dir");

    {
        // Build a pre-WP-103 streaming_translations table by hand, using
        // the old paragraph_key column name, with one row of data.
        let connection = Connection::open(&db_path).expect("open raw pre-rename database");
        connection
                .execute_batch(
                    r#"
                    CREATE TABLE streaming_sessions (
                        id INTEGER PRIMARY KEY,
                        title TEXT NOT NULL,
                        created_at_ms INTEGER NOT NULL,
                        updated_at_ms INTEGER NOT NULL,
                        status TEXT NOT NULL,
                        translation_enabled INTEGER NOT NULL DEFAULT 0
                    );
                    CREATE TABLE streaming_translations (
                        session_id INTEGER NOT NULL REFERENCES streaming_sessions(id) ON DELETE CASCADE,
                        paragraph_key INTEGER NOT NULL,
                        target_language TEXT NOT NULL,
                        source_text TEXT NOT NULL,
                        translated_text TEXT NOT NULL,
                        updated_at_ms INTEGER NOT NULL,
                        PRIMARY KEY (session_id, paragraph_key, target_language)
                    );
                    INSERT INTO streaming_sessions
                        (id, title, created_at_ms, updated_at_ms, status, translation_enabled)
                        VALUES (1, 'Pre-rename session', 100, 200, 'stopped', 0);
                    INSERT INTO streaming_translations
                        (session_id, paragraph_key, target_language, source_text, translated_text, updated_at_ms)
                        VALUES (1, 3, 'en', 'Исходный текст.', 'Source text.', 1000);
                    "#,
                )
                .expect("seed pre-rename schema and data");
    }

    let store = StreamingStore::open(temp.path()).expect("open (and migrate) database");

    let rows = store.list_translations(1, "en").expect("list translations");
    assert_eq!(
        rows,
        vec![StreamingTranslation {
            session_id: 1,
            window_index: 3,
            target_language: "en".to_string(),
            source_text: "Исходный текст.".to_string(),
            translated_text: "Source text.".to_string(),
            updated_at_ms: 1_000,
        }]
    );

    // The column is now writable under its new name, not just readable
    // with data preserved from before the rename.
    store
        .upsert_translation(&StreamingTranslation {
            session_id: 1,
            window_index: 4,
            target_language: "en".to_string(),
            source_text: "Другой текст.".to_string(),
            translated_text: "Other text.".to_string(),
            updated_at_ms: 2_000,
        })
        .expect("upsert into migrated (renamed) column");
    assert_eq!(store.list_translations(1, "en").expect("list").len(), 2);
}

/// Reopening a database whose `streaming_translations` table has already
/// been migrated to `window_index` must not error on a duplicate
/// `RENAME COLUMN` — the column-presence check must make the migration a
/// no-op once the column is already named `window_index`.
#[test]
fn reopening_an_already_window_index_migrated_database_does_not_error() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let session_id;
    {
        let store = StreamingStore::open(temp.path()).expect("first open");
        session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        store
            .upsert_translation(&StreamingTranslation {
                session_id,
                window_index: 0,
                target_language: "en".to_string(),
                source_text: "Привет.".to_string(),
                translated_text: "Hi.".to_string(),
                updated_at_ms: 1_000,
            })
            .expect("upsert translation");
    }

    let reopened =
        StreamingStore::open(temp.path()).expect("reopening an already-migrated database");
    assert_eq!(
        reopened
            .list_translations(session_id, "en")
            .expect("list")
            .len(),
        1
    );
}

// --- WP-101: translation_enabled column, migration, and persistence ---

/// Opening a database whose `streaming_sessions` table predates the
/// `translation_enabled` column must both preserve the existing session
/// row and add the column, defaulted to false (off) — the same
/// check-then-`ALTER TABLE` shape as `migrate_legacy_streaming_notes`,
/// since SQLite's `ADD COLUMN` has no `IF NOT EXISTS` clause to fold into
/// the idempotent `CREATE TABLE IF NOT EXISTS` schema batch.
#[test]
fn opening_a_pre_migration_database_adds_translation_enabled_defaulted_to_false() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let db_path = crate::store::shared_database_path(temp.path());
    std::fs::create_dir_all(temp.path()).expect("create app-support dir");

    {
        // A pre-WP-101 streaming_sessions table: every column this
        // feature's migration must leave intact, deliberately excluding
        // translation_enabled.
        let connection = Connection::open(&db_path).expect("open raw pre-migration database");
        connection
            .execute_batch(
                r#"
                    CREATE TABLE streaming_sessions (
                        id INTEGER PRIMARY KEY,
                        title TEXT NOT NULL,
                        created_at_ms INTEGER NOT NULL,
                        updated_at_ms INTEGER NOT NULL,
                        status TEXT NOT NULL
                    );
                    INSERT INTO streaming_sessions (id, title, created_at_ms, updated_at_ms, status)
                        VALUES (1, 'Pre-migration session', 100, 200, 'stopped');
                    "#,
            )
            .expect("seed pre-migration schema and data");
    }

    let store = StreamingStore::open(temp.path()).expect("open (and migrate) database");

    let session = store
        .get_session(1)
        .expect("get session")
        .expect("session survives migration");
    assert_eq!(session.title, "Pre-migration session");
    assert_eq!(session.status, status::STOPPED);
    assert!(
        !session.translation_enabled,
        "a pre-existing session must default to translation_enabled = false"
    );
    assert_eq!(session.translation_target_language, "ru");

    // The column is now writable, not just readable with a default.
    store
        .set_translation_enabled(1, true)
        .expect("set translation_enabled on the migrated column");
    assert!(
        store
            .get_session(1)
            .expect("get session")
            .expect("session exists")
            .translation_enabled
    );
}

/// Reopening the (already-migrated) database a second time must not
/// error on a duplicate `ALTER TABLE ADD COLUMN` — the column-presence
/// check must make the migration a no-op once the column exists.
#[test]
fn reopening_an_already_migrated_database_does_not_error() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    {
        let store = StreamingStore::open(temp.path()).expect("first open");
        store.create_session(draft("Standup", 100)).unwrap();
    }

    StreamingStore::open(temp.path()).expect("reopening an already-migrated database");
}

#[test]
fn set_translation_enabled_persists_and_is_readable_after_reopen() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let session_id;
    {
        let store = StreamingStore::open(temp.path()).expect("open database");
        session_id = store.create_session(draft("Standup", 100)).unwrap().id;
        assert!(
            !store
                .get_session(session_id)
                .unwrap()
                .unwrap()
                .translation_enabled
        );

        store
            .set_translation_enabled(session_id, true)
            .expect("set translation_enabled");
    }

    let reopened = StreamingStore::open(temp.path()).expect("reopen database");
    assert!(
        reopened
            .get_session(session_id)
            .expect("get session")
            .expect("session exists")
            .translation_enabled
    );
}

#[test]
fn set_translation_enabled_back_to_false_overwrites_the_prior_value() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let session_id = store.create_session(draft("Standup", 100)).unwrap().id;
    store
        .set_translation_enabled(session_id, true)
        .expect("enable");

    store
        .set_translation_enabled(session_id, false)
        .expect("disable");

    assert!(
        !store
            .get_session(session_id)
            .expect("get session")
            .expect("session exists")
            .translation_enabled
    );
}

#[test]
fn setting_translation_enabled_for_an_unknown_session_is_a_store_error() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");

    let result = store.set_translation_enabled(999_999, true);

    assert!(matches!(result, Err(AppError::Store(_))));
}

#[test]
fn two_sessions_have_independent_translation_enabled_values() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = StreamingStore::open(temp.path()).expect("open database");
    let a = store.create_session(draft("A", 100)).unwrap().id;
    let b = store.create_session(draft("B", 200)).unwrap().id;

    store.set_translation_enabled(a, true).expect("enable a");

    assert!(store.get_session(a).unwrap().unwrap().translation_enabled);
    assert!(!store.get_session(b).unwrap().unwrap().translation_enabled);
}

#[test]
fn translation_target_language_is_scoped_to_and_persisted_with_its_session() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let a;
    let b;
    {
        let store = StreamingStore::open(temp.path()).expect("open database");
        a = store.create_session(draft("A", 100)).unwrap().id;
        b = store.create_session(draft("B", 200)).unwrap().id;
        store
            .set_translation_target_language(a, "en")
            .expect("persist English target");
    }

    let reopened = StreamingStore::open(temp.path()).expect("reopen database");
    assert_eq!(
        reopened
            .get_session(a)
            .unwrap()
            .unwrap()
            .translation_target_language,
        "en"
    );
    assert_eq!(
        reopened
            .get_session(b)
            .unwrap()
            .unwrap()
            .translation_target_language,
        "ru"
    );
}

#[test]
fn target_language_migration_uses_the_latest_existing_translation() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let db_path = crate::store::shared_database_path(temp.path());
    std::fs::create_dir_all(temp.path()).expect("create app-support dir");
    let connection = Connection::open(&db_path).expect("open legacy database");
    connection
            .execute_batch(
                r#"
                CREATE TABLE streaming_sessions (
                    id INTEGER PRIMARY KEY,
                    title TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    translation_enabled INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE streaming_translations (
                    session_id INTEGER NOT NULL,
                    window_index INTEGER NOT NULL,
                    target_language TEXT NOT NULL,
                    source_text TEXT NOT NULL,
                    translated_text TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    PRIMARY KEY (session_id, window_index, target_language)
                );
                INSERT INTO streaming_sessions
                    (id, title, created_at_ms, updated_at_ms, status, translation_enabled)
                    VALUES (1, 'Duo', 100, 200, 'stopped', 1);
                INSERT INTO streaming_translations
                    (session_id, window_index, target_language, source_text, translated_text, updated_at_ms)
                    VALUES (1, 0, 'ru', 'Hello', 'Привет', 200);
                INSERT INTO streaming_translations
                    (session_id, window_index, target_language, source_text, translated_text, updated_at_ms)
                    VALUES (1, 0, 'en', 'Привет', 'Hello', 300);
                "#,
            )
            .expect("seed session with an English translation");
    drop(connection);

    let store = StreamingStore::open(temp.path()).expect("migrate database");

    assert_eq!(
        store
            .get_session(1)
            .unwrap()
            .unwrap()
            .translation_target_language,
        "en"
    );
}
