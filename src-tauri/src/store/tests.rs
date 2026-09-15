use super::*;
use crate::error::AppError;

fn draft(title: &str, created_at_ms: i64) -> NewMeeting {
    NewMeeting {
        title: title.to_string(),
        source_path: Some(format!("/recordings/{title}.m4a")),
        source_name: Some(format!("{title}.m4a")),
        created_at_ms,
        duration_ms: Some(42_000),
        language: "ru".to_string(),
        status: "ready".to_string(),
    }
}

fn segment(ordinal: i64, start_ms: i64, end_ms: i64) -> NewSegment {
    NewSegment {
        ordinal,
        start_ms,
        end_ms,
        text: format!("segment {ordinal}"),
        speaker_id: Some(ordinal + 1),
    }
}

fn mfu(meeting_id: MeetingId) -> MeetingMfu {
    MeetingMfu {
        meeting_id,
        summary: "Summary".to_string(),
        decisions: "Decision".to_string(),
        action_items: "Action".to_string(),
        open_questions: "Question".to_string(),
        participants: "Participant".to_string(),
    }
}

#[test]
fn given_empty_directory_when_creating_meeting_then_it_persists_and_lists() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = Store::open(temp.path()).expect("open database");

    let created = store
        .create_meeting(draft("Planning", 100))
        .expect("create meeting");

    assert_eq!(
        store.get_meeting(created.id).expect("open meeting"),
        Some(created.clone())
    );
    assert_eq!(
        store.list_meetings().expect("list meetings"),
        vec![MeetingSummary {
            id: created.id,
            title: "Planning".to_string(),
            created_at_ms: 100,
            duration_ms: Some(42_000),
            status: "ready".to_string(),
        }]
    );

    let mut renamed = created;
    renamed.title = "Updated planning".to_string();
    renamed.status = "finished".to_string();
    store.update_meeting(&renamed).expect("update meeting");
    assert_eq!(
        store.get_meeting(renamed.id).expect("open updated meeting"),
        Some(renamed)
    );
}

#[test]
fn given_saved_data_when_reopened_then_meeting_segments_mfu_and_newest_summary_persist() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let first_id;
    let second_id;

    {
        let store = Store::open(temp.path()).expect("open database");
        first_id = store
            .create_meeting(draft("Earlier", 100))
            .expect("create first")
            .id;
        second_id = store
            .create_meeting(draft("Later", 200))
            .expect("create second")
            .id;
        store
            .replace_segments(second_id, &[segment(2, 1_000, 2_000), segment(1, 0, 900)])
            .expect("replace segments");
        store.upsert_mfu(&mfu(second_id)).expect("upsert mfu");
    }

    let reopened = Store::open(temp.path()).expect("reopen database");
    assert_eq!(
        reopened
            .list_meetings()
            .expect("list summaries")
            .into_iter()
            .map(|m| m.id)
            .collect::<Vec<_>>(),
        vec![second_id, first_id]
    );
    assert_eq!(
        reopened
            .list_segments(second_id)
            .expect("list segments")
            .into_iter()
            .map(|s| s.ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        reopened.get_mfu(second_id).expect("get mfu"),
        Some(mfu(second_id))
    );
}

#[test]
fn recovers_mixed_mfu_schema_with_orphaned_legacy_rows() {
    // Decision table: notes-only migrates; mixed schemas preserve current rows, import valid legacy rows, and discard orphans.
    let legacy_only = tempfile::tempdir().expect("legacy-only app support");
    let connection = Connection::open(database_path(legacy_only.path())).expect("open legacy db");
    connection
            .execute_batch(
                "CREATE TABLE meetings (id INTEGER PRIMARY KEY, title TEXT NOT NULL, source_path TEXT, source_name TEXT, created_at_ms INTEGER NOT NULL, duration_ms INTEGER, language TEXT NOT NULL, status TEXT NOT NULL);
                 CREATE TABLE notes (meeting_id INTEGER PRIMARY KEY, summary TEXT NOT NULL, decisions TEXT NOT NULL, action_items TEXT NOT NULL, open_questions TEXT NOT NULL, participants TEXT NOT NULL);
                 INSERT INTO meetings VALUES (1, 'Legacy', NULL, NULL, 1, NULL, 'en', 'finished');
                 INSERT INTO notes VALUES (1, 'Legacy summary', 'Legacy decisions', 'Legacy actions', 'Legacy questions', 'Legacy participants');",
            )
            .expect("seed legacy db");
    drop(connection);

    let legacy_store = Store::open(legacy_only.path()).expect("migrate legacy db");
    assert_eq!(
        legacy_store.get_mfu(1).expect("read migrated mfu"),
        Some(MeetingMfu {
            meeting_id: 1,
            summary: "Legacy summary".to_string(),
            decisions: "Legacy decisions".to_string(),
            action_items: "Legacy actions".to_string(),
            open_questions: "Legacy questions".to_string(),
            participants: "Legacy participants".to_string(),
        })
    );
    drop(legacy_store);
    let reopened_legacy = Store::open(legacy_only.path()).expect("repeat legacy migration");
    assert_eq!(
        reopened_legacy
            .get_mfu(1)
            .expect("read reopened legacy mfu"),
        Some(MeetingMfu {
            meeting_id: 1,
            summary: "Legacy summary".to_string(),
            decisions: "Legacy decisions".to_string(),
            action_items: "Legacy actions".to_string(),
            open_questions: "Legacy questions".to_string(),
            participants: "Legacy participants".to_string(),
        })
    );
    drop(reopened_legacy);
    let legacy_notes_exist = Connection::open(database_path(legacy_only.path()))
        .expect("reopen legacy db")
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'notes')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("read legacy schema");
    assert!(!legacy_notes_exist);

    let mixed = tempfile::tempdir().expect("mixed-schema app support");
    let connection = Connection::open(database_path(mixed.path())).expect("open mixed db");
    connection
            .execute_batch(
                "CREATE TABLE meetings (id INTEGER PRIMARY KEY, title TEXT NOT NULL, source_path TEXT, source_name TEXT, created_at_ms INTEGER NOT NULL, duration_ms INTEGER, language TEXT NOT NULL, status TEXT NOT NULL);
                 CREATE TABLE notes (meeting_id INTEGER PRIMARY KEY, summary TEXT NOT NULL, decisions TEXT NOT NULL, action_items TEXT NOT NULL, open_questions TEXT NOT NULL, participants TEXT NOT NULL);
                 CREATE TABLE mfu (meeting_id INTEGER PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE, summary TEXT NOT NULL, decisions TEXT NOT NULL, action_items TEXT NOT NULL, open_questions TEXT NOT NULL, participants TEXT NOT NULL);
                 INSERT INTO meetings VALUES (1, 'Current', NULL, NULL, 1, NULL, 'en', 'finished'), (2, 'Legacy', NULL, NULL, 2, NULL, 'en', 'finished');
                 INSERT INTO mfu VALUES (1, 'Current summary', 'Current decisions', 'Current actions', 'Current questions', 'Current participants');
                 INSERT INTO notes VALUES (1, 'Stale summary', 'Stale decisions', 'Stale actions', 'Stale questions', 'Stale participants'), (2, 'Imported summary', 'Imported decisions', 'Imported actions', 'Imported questions', 'Imported participants'), (3, 'Orphaned summary', 'Orphaned decisions', 'Orphaned actions', 'Orphaned questions', 'Orphaned participants');",
            )
            .expect("seed mixed db");
    drop(connection);

    let mixed_store = Store::open(mixed.path()).expect("recover mixed db");
    assert_eq!(
        mixed_store.get_mfu(1).expect("read current mfu"),
        Some(MeetingMfu {
            meeting_id: 1,
            summary: "Current summary".to_string(),
            decisions: "Current decisions".to_string(),
            action_items: "Current actions".to_string(),
            open_questions: "Current questions".to_string(),
            participants: "Current participants".to_string(),
        })
    );
    assert_eq!(
        mixed_store.get_mfu(2).expect("read imported mfu"),
        Some(MeetingMfu {
            meeting_id: 2,
            summary: "Imported summary".to_string(),
            decisions: "Imported decisions".to_string(),
            action_items: "Imported actions".to_string(),
            open_questions: "Imported questions".to_string(),
            participants: "Imported participants".to_string(),
        })
    );
    assert_eq!(mixed_store.get_mfu(3).expect("read orphaned mfu"), None);
    drop(mixed_store);
    let reopened_mixed = Store::open(mixed.path()).expect("repeat mixed migration");
    assert_eq!(
        reopened_mixed
            .get_mfu(1)
            .expect("read reopened current mfu"),
        Some(MeetingMfu {
            meeting_id: 1,
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
        Some(MeetingMfu {
            meeting_id: 2,
            summary: "Imported summary".to_string(),
            decisions: "Imported decisions".to_string(),
            action_items: "Imported actions".to_string(),
            open_questions: "Imported questions".to_string(),
            participants: "Imported participants".to_string(),
        })
    );
    drop(reopened_mixed);
    let mixed_notes_exist = Connection::open(database_path(mixed.path()))
        .expect("reopen mixed db")
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'notes')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("read mixed schema");
    assert!(!mixed_notes_exist);
}

#[test]
fn given_invalid_or_duplicate_segments_when_replacing_then_store_error_leaves_no_partial_rows() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = Store::open(temp.path()).expect("open database");
    let meeting_id = store
        .create_meeting(draft("Invalid", 100))
        .expect("create meeting")
        .id;

    store
        .replace_segments(meeting_id, &[segment(5, 0, 100)])
        .expect("save initial segment");
    let expected_segments = store
        .list_segments(meeting_id)
        .expect("list initial segment");

    // EP: a reversed range is invalid while a non-negative ordered range is valid.
    let invalid_range = store.replace_segments(meeting_id, &[segment(0, 2_000, 1_000)]);
    assert!(matches!(invalid_range, Err(AppError::Store(_))));
    assert_eq!(
        store.list_segments(meeting_id).expect("list segments"),
        expected_segments
    );

    // EP: duplicate and distinct ordinals are separate persistence classes.
    let duplicate_ordinal =
        store.replace_segments(meeting_id, &[segment(0, 0, 100), segment(0, 200, 300)]);
    assert!(matches!(duplicate_ordinal, Err(AppError::Store(_))));
    assert_eq!(
        store.list_segments(meeting_id).expect("list segments"),
        expected_segments
    );

    // EP: an existing and an unknown meeting id must not share a write path.
    let unknown_meeting = store.replace_segments(meeting_id + 1_000, &[segment(0, 0, 100)]);
    assert!(matches!(unknown_meeting, Err(AppError::Store(_))));
}

#[test]
fn given_meeting_with_dependents_when_deleted_then_segments_and_mfu_are_cascaded() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = Store::open(temp.path()).expect("open database");
    let meeting_id = store
        .create_meeting(draft("Disposable", 100))
        .expect("create meeting")
        .id;
    store
        .replace_segments(meeting_id, &[segment(0, 0, 100)])
        .expect("replace segments");
    store.upsert_mfu(&mfu(meeting_id)).expect("upsert mfu");

    store.delete_meeting(meeting_id).expect("delete meeting");

    assert_eq!(store.get_meeting(meeting_id).expect("get meeting"), None);
    assert!(store
        .list_segments(meeting_id)
        .expect("list segments")
        .is_empty());
    assert_eq!(store.get_mfu(meeting_id).expect("get mfu"), None);
}

#[test]
fn given_saved_mfu_when_deleted_then_only_the_mfu_record_is_removed() {
    let temp = tempfile::tempdir().expect("temporary app-support directory");
    let store = Store::open(temp.path()).expect("open database");
    let meeting_id = store
        .create_meeting(draft("MFU", 100))
        .expect("create meeting")
        .id;
    store.upsert_mfu(&mfu(meeting_id)).expect("upsert mfu");

    store.delete_mfu(meeting_id).expect("delete mfu");

    assert_eq!(store.get_mfu(meeting_id).expect("get mfu"), None);
    assert!(store
        .get_meeting(meeting_id)
        .expect("get meeting")
        .is_some());
}
