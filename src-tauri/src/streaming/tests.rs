use super::*;
use crate::cloud_provider::CloudProvider;
use crate::streaming_store::NewStreamingWindow;

#[test]
fn given_no_sessions_when_listing_then_result_is_empty() {
    let temp = tempfile::tempdir().expect("temp dir");
    assert!(list_streaming_sessions(temp.path())
        .expect("list")
        .is_empty());
}

#[test]
fn create_then_open_round_trips_with_no_windows() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let dto = open_streaming_session(temp.path(), id).expect("open");

    assert_eq!(dto.id, id);
    assert_eq!(dto.title, "New Meeting");
    assert_eq!(dto.status, streaming_store::status::STOPPED);
    assert!(dto.windows.is_empty());
}

#[test]
fn opening_an_unknown_session_is_a_store_error() {
    let temp = tempfile::tempdir().expect("temp dir");
    assert!(open_streaming_session(temp.path(), 999_999).is_err());
}

#[test]
fn rename_then_open_reflects_the_new_title() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let dto =
        rename_streaming_session(temp.path(), id, "Team standup".to_string()).expect("rename");

    assert_eq!(dto.title, "Team standup");
}

#[test]
fn delete_then_open_reports_not_found() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    delete_streaming_session(temp.path(), id).expect("delete");

    assert!(open_streaming_session(temp.path(), id).is_err());
}

// S-1: happy path — a stopped session with saved windows resumes from
// one past the last window index, and its status/freshness update.
#[test]
fn resuming_a_stopped_session_with_windows_continues_from_the_next_index() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = StreamingStore::open(temp.path()).expect("open store");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    store
        .append_window(
            id,
            &NewStreamingWindow {
                window_index: 0,
                start_ms: 0,
                end_ms: 7_000,
                text: "hello".to_string(),
                language: "en".to_string(),
                outcome_ok: true,
            },
            7_100,
        )
        .expect("append window 0");
    store
        .append_window(
            id,
            &NewStreamingWindow {
                window_index: 1,
                start_ms: 7_000,
                end_ms: 42_000,
                text: "there".to_string(),
                language: "en".to_string(),
                outcome_ok: true,
            },
            42_100,
        )
        .expect("append window 1");
    store.mark_stopped(id, 43_000).expect("mark stopped");

    let (summary, resume) = resume_streaming_session(temp.path(), id, 20_000).expect("resume");

    assert_eq!(resume.next_window_index, 2);
    assert_eq!(resume.last_persisted_end_ms, 42_000);
    assert_eq!(summary.status, streaming_store::status::ACTIVE);
    assert_eq!(summary.updated_at_ms, 20_000);
    assert_eq!(summary.title, "New Meeting");
    let reloaded = store
        .get_session(id)
        .expect("get session")
        .expect("session exists");
    assert_eq!(reloaded.status, streaming_store::status::ACTIVE);
}

// BVA: a stopped session that never saved a window resumes at index 0,
// the lower boundary — not an out-of-range or panicking `.last()`.
#[test]
fn resuming_a_stopped_session_with_no_windows_starts_at_index_zero() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = StreamingStore::open(temp.path()).expect("open store");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    store.mark_stopped(id, 500).expect("mark stopped");

    let (_summary, resume) = resume_streaming_session(temp.path(), id, 600).expect("resume");

    assert_eq!(resume.next_window_index, 0);
    assert_eq!(resume.last_persisted_end_ms, 0);
}

#[test]
fn resumed_cloud_timestamps_advance_after_the_persisted_timeline() {
    assert_eq!(
        resumed_cloud_window_bounds(42_000, 42_000, 3_000),
        (42_000, 45_000)
    );
    assert_eq!(
        resumed_cloud_window_bounds(42_000, 45_000, 3_000),
        (45_000, 45_001),
        "a repeated provider timestamp still creates a strictly positive window"
    );
    assert_eq!(
        resumed_cloud_window_bounds(0, 0, -10),
        (0, 1),
        "negative provider timestamps cannot move the session backwards"
    );
}

// S-2, decision-table: status=active is the one rejected cell — resuming
// an already-running session would double-capture into it.
#[test]
fn resuming_an_active_session_is_rejected() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    StreamingStore::open(temp.path())
        .expect("open store")
        .mark_active(id, 150)
        .expect("mark active");

    assert!(resume_streaming_session(temp.path(), id, 200).is_err());
}

#[test]
fn resuming_a_nonexistent_session_is_a_store_error() {
    let temp = tempfile::tempdir().expect("temp dir");

    assert!(resume_streaming_session(temp.path(), 999_999, 100).is_err());
}

#[test]
fn prepare_start_persists_cloud_provider_once_and_reuses_it_on_resume() {
    let temp = tempfile::tempdir().expect("temporary app support");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    let cloud = StreamingStartConfiguration::Cloud(CloudProvider::OpenAi);

    let (first, _, stored) =
        prepare_streaming_session_start(temp.path(), id, Some(cloud), 200).expect("prepare cloud");
    assert_eq!(first.status, streaming_store::status::ACTIVE);
    assert_eq!(stored, cloud);

    StreamingStore::open(temp.path())
        .expect("open store")
        .mark_stopped(id, 300)
        .expect("stop");
    let (_, _, resumed) =
        prepare_streaming_session_start(temp.path(), id, None, 400).expect("resume cloud");
    assert_eq!(resumed, cloud);
}

#[test]
fn prepare_start_reuses_the_persisted_configuration_when_a_resume_request_changes() {
    let temp = tempfile::tempdir().expect("temporary app support");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    prepare_streaming_session_start(
        temp.path(),
        id,
        Some(StreamingStartConfiguration::Cloud(CloudProvider::Deepgram)),
        200,
    )
    .expect("prepare cloud");
    StreamingStore::open(temp.path())
        .expect("open store")
        .mark_stopped(id, 300)
        .expect("stop");

    let (_, _, stored) = prepare_streaming_session_start(
        temp.path(),
        id,
        Some(StreamingStartConfiguration::Local),
        400,
    )
    .expect("resume must retain immutable configuration");
    assert_eq!(
        stored,
        StreamingStartConfiguration::Cloud(CloudProvider::Deepgram)
    );
}

#[test]
fn open_session_exposes_its_non_secret_engine_for_resume_selection() {
    let temp = tempfile::tempdir().expect("temporary app support");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    prepare_streaming_session_start(
        temp.path(),
        id,
        Some(StreamingStartConfiguration::Cloud(CloudProvider::OpenAi)),
        200,
    )
    .expect("prepare cloud");
    StreamingStore::open(temp.path())
        .expect("open store")
        .mark_stopped(id, 300)
        .expect("stop");

    let dto = open_streaming_session(temp.path(), id).expect("open");

    assert_eq!(dto.transcription_engine.as_deref(), Some("cloud"));
}

#[test]
fn open_returns_windows_in_order_with_all_fields() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    let store = StreamingStore::open(temp.path()).expect("open store");
    store
        .append_window(
            id,
            &NewStreamingWindow {
                window_index: 0,
                start_ms: 0,
                end_ms: 7_000,
                text: "hello".to_string(),
                language: "en".to_string(),
                outcome_ok: true,
            },
            7_100,
        )
        .expect("append window");

    let dto = open_streaming_session(temp.path(), id).expect("open");

    assert_eq!(
        dto.windows,
        vec![StreamingWindowDto {
            window_index: 0,
            start_ms: 0,
            end_ms: 7_000,
            text: "hello".to_string(),
            language: "en".to_string(),
            outcome_ok: true,
        }]
    );
}

fn stopped_session_with_windows(
    temp: &std::path::Path,
    windows: &[NewStreamingWindow],
) -> StreamingSessionId {
    let store = StreamingStore::open(temp).expect("open store");
    let id = create_streaming_session(temp, 100).expect("create");
    for (i, w) in windows.iter().enumerate() {
        store
            .append_window(id, w, 100 + i as i64)
            .expect("append window");
    }
    store.mark_stopped(id, 999).expect("mark stopped");
    id
}

fn ok_window(index: i64, text: &str) -> NewStreamingWindow {
    NewStreamingWindow {
        window_index: index,
        start_ms: index * 7_000,
        end_ms: (index + 1) * 7_000,
        text: text.to_string(),
        language: "en".to_string(),
        outcome_ok: true,
    }
}

fn failed_window(index: i64) -> NewStreamingWindow {
    NewStreamingWindow {
        window_index: index,
        start_ms: index * 7_000,
        end_ms: (index + 1) * 7_000,
        text: String::new(),
        language: "auto".to_string(),
        outcome_ok: false,
    }
}

// S-5, EP: outcome_ok=true vs false windows are two input partitions.
#[test]
fn build_transcript_joins_only_ok_windows_in_order() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = stopped_session_with_windows(
        temp.path(),
        &[
            ok_window(1, "second"),
            failed_window(0),
            ok_window(2, "third"),
        ],
    );

    let transcript = build_streaming_transcript(temp.path(), id).expect("transcript");

    assert_eq!(transcript, "second third");
}

// S-12, decision-table: status=active is the one rejected cell.
#[test]
fn build_transcript_errors_on_active_session() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = StreamingStore::open(temp.path()).expect("open store");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    store.mark_active(id, 150).expect("mark active");
    store
        .append_window(id, &ok_window(0, "hello"), 200)
        .expect("append window");

    assert!(build_streaming_transcript(temp.path(), id).is_err());
}

// BVA: zero windows is the lower boundary of "no transcript".
#[test]
fn build_transcript_errors_on_no_windows() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = stopped_session_with_windows(temp.path(), &[]);

    assert!(build_streaming_transcript(temp.path(), id).is_err());
}

// S-6, BVA: all windows present but all outcome_ok=false — the boundary
// just past "one ok window" (build_transcript_joins_only_ok_windows).
#[test]
fn build_transcript_errors_when_all_windows_failed() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = stopped_session_with_windows(temp.path(), &[failed_window(0), failed_window(1)]);

    assert!(build_streaming_transcript(temp.path(), id).is_err());
}

#[test]
fn build_transcript_errors_on_nonexistent_session() {
    let temp = tempfile::tempdir().expect("temp dir");

    assert!(build_streaming_transcript(temp.path(), 999_999).is_err());
}

#[test]
fn open_streaming_session_notes_is_none_when_absent() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let dto = open_streaming_session(temp.path(), id).expect("open");

    assert_eq!(dto.mfu, None);
}

#[test]
fn open_streaming_session_includes_notes_when_present() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = StreamingStore::open(temp.path()).expect("open store");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    store
        .append_window(id, &ok_window(0, "Transcript."), 200)
        .expect("append window");
    store
        .upsert_mfu(&crate::streaming_store::StreamingMfu {
            session_id: id,
            summary: "Summary.".to_string(),
            decisions: "Decisions.".to_string(),
            action_items: "Actions.".to_string(),
            open_questions: "Questions.".to_string(),
            participants: "Alex".to_string(),
        })
        .expect("upsert mfu");

    let dto = open_streaming_session(temp.path(), id).expect("open");

    assert_eq!(
        dto.mfu,
        Some(StreamingMfuDto {
            summary: "Summary.".to_string(),
            decisions: "Decisions.".to_string(),
            action_items: "Actions.".to_string(),
            open_questions: "Questions.".to_string(),
            participants: "Alex".to_string(),
        })
    );
}

#[test]
fn open_streaming_session_prettified_text_is_none_when_absent() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let dto = open_streaming_session(temp.path(), id).expect("open");

    assert_eq!(dto.prettified_text, None);
}

#[test]
fn open_streaming_session_includes_prettified_text_when_present() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = StreamingStore::open(temp.path()).expect("open store");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    store
        .append_window(id, &ok_window(0, "Transcript."), 200)
        .expect("append window");
    store
        .upsert_prettified(id, "Cleaned transcript.")
        .expect("upsert prettified");

    let dto = open_streaming_session(temp.path(), id).expect("open");

    assert_eq!(dto.prettified_text, Some("Cleaned transcript.".to_string()));
}

#[test]
fn clearing_a_stopped_session_removes_all_derived_content_but_keeps_configuration() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = StreamingStore::open(temp.path()).expect("open store");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    store
        .append_window(id, &ok_window(0, "Saved transcript."), 200)
        .expect("append window");
    store
        .upsert_mfu(&crate::streaming_store::StreamingMfu {
            session_id: id,
            summary: "Summary".to_string(),
            decisions: String::new(),
            action_items: String::new(),
            open_questions: String::new(),
            participants: String::new(),
        })
        .expect("save mfu");
    store
        .upsert_prettified(id, "Prettified")
        .expect("save prettified");
    store
        .set_translation_enabled(id, true)
        .expect("enable translation");
    store
        .upsert_translation(&crate::streaming_store::StreamingTranslation {
            session_id: id,
            window_index: 0,
            target_language: "ru".to_string(),
            source_text: "Saved transcript.".to_string(),
            translated_text: "Сохранённый текст.".to_string(),
            updated_at_ms: 200,
        })
        .expect("save translation");
    store
        .set_session_configuration(
            id,
            &crate::streaming_store::StreamingSessionConfiguration {
                engine: "local".to_string(),
                cloud_provider: None,
                cloud_model: None,
            },
        )
        .expect("save configuration");

    let cleared = clear_streaming_session(temp.path(), id).expect("clear session");

    assert!(cleared.windows.is_empty());
    assert_eq!(cleared.mfu, None);
    assert_eq!(cleared.prettified_text, None);
    assert!(cleared.translation_enabled);
    assert_eq!(cleared.transcription_engine.as_deref(), Some("local"));
    assert_eq!(
        store
            .list_sessions()
            .expect("list sessions")
            .into_iter()
            .find(|session| session.id == id)
            .expect("cleared summary")
            .updated_at_ms,
        100
    );
    assert!(list_streaming_translations(temp.path(), id, "ru")
        .expect("list translations")
        .is_empty());
    assert!(store.upsert_prettified(id, "late prettify").is_err());
    assert!(store
        .upsert_mfu(&crate::streaming_store::StreamingMfu {
            session_id: id,
            summary: "late MFU".to_string(),
            decisions: String::new(),
            action_items: String::new(),
            open_questions: String::new(),
            participants: String::new(),
        })
        .is_err());

    let (_summary, resume) =
        resume_streaming_session(temp.path(), id, 50_000).expect("resume cleared session");
    store
        .append_window(
            id,
            &NewStreamingWindow {
                window_index: resume.next_window_index as i64,
                start_ms: 0,
                end_ms: 7_000,
                text: "New recording.".to_string(),
                language: "en".to_string(),
                outcome_ok: true,
            },
            57_000,
        )
        .expect("append after clear");
    let resumed_summary = store
        .list_sessions()
        .expect("list sessions")
        .into_iter()
        .find(|session| session.id == id)
        .expect("resumed summary");
    assert_eq!(resumed_summary.duration_ms, 7_000);
}

#[test]
fn clearing_an_active_streaming_session_is_rejected() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = StreamingStore::open(temp.path()).expect("open store");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    store.mark_active(id, 200).expect("mark active");

    assert!(clear_streaming_session(temp.path(), id).is_err());
}

#[test]
fn streaming_session_dto_round_trips_through_the_ipc_json_contract() {
    let original = StreamingSessionDto {
        id: 7,
        title: "Contract session".to_string(),
        created_at_ms: 42,
        updated_at_ms: 100,
        status: "stopped".to_string(),
        windows: vec![StreamingWindowDto {
            window_index: 0,
            start_ms: 0,
            end_ms: 7_000,
            text: "Saved window".to_string(),
            language: "en".to_string(),
            outcome_ok: true,
        }],
        mfu: Some(StreamingMfuDto {
            summary: "Summary.".to_string(),
            decisions: "Decisions.".to_string(),
            action_items: "Actions.".to_string(),
            open_questions: "Questions.".to_string(),
            participants: "Alex".to_string(),
        }),
        prettified_text: Some("Cleaned transcript.".to_string()),
        translation_enabled: false,
        translation_target_language: "ru".to_string(),
        transcription_engine: Some("cloud".to_string()),
    };

    let json = serde_json::to_value(&original).expect("serialize streaming session DTO");
    assert_eq!(json["transcription_engine"], "cloud");
    let round_tripped: StreamingSessionDto =
        serde_json::from_value(json).expect("deserialize streaming session DTO");

    assert_eq!(round_tripped, original);
}

// --- WP-92: translate_streaming_window's testable core ---

#[test]
fn ensure_translation_request_is_valid_rejects_an_unsupported_target_language() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let result = ensure_translation_request_is_valid(temp.path(), id, "fr", "Bonjour");

    assert!(matches!(result, Err(AppError::Llm(_))));
}

#[test]
fn ensure_translation_request_is_valid_rejects_an_unknown_session() {
    let temp = tempfile::tempdir().expect("temp dir");

    let result = ensure_translation_request_is_valid(temp.path(), 999_999, "en", "Привет");

    assert!(matches!(result, Err(AppError::Store(_))));
}

#[test]
fn ensure_translation_request_is_valid_rejects_empty_source_text() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let result = ensure_translation_request_is_valid(temp.path(), id, "en", "   \n  ");

    assert!(matches!(result, Err(AppError::Llm(_))));
}

#[test]
fn ensure_translation_request_is_valid_accepts_a_well_formed_request() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let result = ensure_translation_request_is_valid(temp.path(), id, "en", "Привет, мир.");

    assert!(result.is_ok());
}

#[test]
fn translate_and_store_persists_on_success_and_returns_translated_text() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let translated = translate_and_store(
        temp.path(),
        id,
        0,
        "en",
        "Привет, мир.",
        None,
        1_000,
        |_text, _lang, _ctx| Ok("Hello, world.".to_string()),
    )
    .expect("translate and store");

    assert_eq!(translated, "Hello, world.");
    let store = StreamingStore::open(temp.path()).expect("open store");
    let rows = store
        .list_translations(id, "en")
        .expect("list translations");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source_text, "Привет, мир.");
    assert_eq!(rows[0].translated_text, "Hello, world.");
}

#[test]
fn translate_and_store_writes_no_row_when_translation_is_rejected() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let result = translate_and_store(
        temp.path(),
        id,
        0,
        "en",
        "Привет, мир.",
        None,
        1_000,
        |_text, _lang, _ctx| Err(AppError::Llm("candidate rejected".into())),
    );

    assert!(result.is_err());
    let store = StreamingStore::open(temp.path()).expect("open store");
    assert!(store
        .list_translations(id, "en")
        .expect("list translations")
        .is_empty());
}

#[test]
fn translate_and_store_overwrites_rather_than_duplicates_for_the_same_window_index() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    translate_and_store(
        temp.path(),
        id,
        0,
        "en",
        "Привет, мир.",
        None,
        1_000,
        |_, _, _| Ok("Hello, world.".to_string()),
    )
    .expect("first translate");
    translate_and_store(
        temp.path(),
        id,
        0,
        "en",
        "Привет, мир.",
        None,
        2_000,
        |_, _, _| Ok("Hi, world.".to_string()),
    )
    .expect("retranslate");

    let store = StreamingStore::open(temp.path()).expect("open store");
    let rows = store
        .list_translations(id, "en")
        .expect("list translations");
    assert_eq!(rows.len(), 1, "retranslation must overwrite, not duplicate");
    assert_eq!(rows[0].translated_text, "Hi, world.");
}

// WP-100: translate_and_store threads its `context` parameter straight
// through to the injected `translate` closure, unchanged.
#[test]
fn translate_and_store_passes_context_through_to_translate() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    let mut received_context: Option<String> = None;

    translate_and_store(
        temp.path(),
        id,
        0,
        "en",
        "Привет, мир.",
        Some("Previous paragraph translation."),
        1_000,
        |_text, _lang, ctx| {
            received_context = ctx.map(|c| c.to_string());
            Ok("Hello, world.".to_string())
        },
    )
    .expect("translate and store");

    assert_eq!(
        received_context.as_deref(),
        Some("Previous paragraph translation.")
    );
}

// WP-100: absent context (None) must reach the translate closure as
// None too, unchanged from the pre-WP-100 behavior.
#[test]
fn translate_and_store_passes_no_context_when_absent() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    let mut received_context: Option<String> = Some("sentinel".to_string());

    translate_and_store(
        temp.path(),
        id,
        0,
        "en",
        "Привет, мир.",
        None,
        1_000,
        |_text, _lang, ctx| {
            received_context = ctx.map(|c| c.to_string());
            Ok("Hello, world.".to_string())
        },
    )
    .expect("translate and store");

    assert!(received_context.is_none());
}

#[test]
fn production_translation_does_not_persist_after_toggle_cancellation() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    let store = StreamingStore::open(temp.path()).expect("open store");
    store
        .append_window(
            id,
            &NewStreamingWindow {
                window_index: 0,
                start_ms: 0,
                end_ms: 1_000,
                text: "Привет, мир.".to_string(),
                language: "ru".to_string(),
                outcome_ok: true,
            },
            200,
        )
        .expect("append source");
    set_streaming_translation_enabled(temp.path(), id, true).expect("enable translation");

    let error = translate_and_store_if_current(
        temp.path(),
        id,
        0,
        "en",
        "Привет, мир.",
        None,
        1_000,
        |_, _, _| {
            set_streaming_translation_enabled(temp.path(), id, false)
                .expect("cancel while inference is in flight");
            Ok("Hello, world.".to_string())
        },
    )
    .expect_err("cancelled work must not persist");

    assert!(matches!(error, AppError::Llm(_)));
    assert!(store
        .list_translations(id, "en")
        .expect("list translations")
        .is_empty());
}

#[test]
fn production_translation_does_not_persist_after_source_revision() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    let store = StreamingStore::open(temp.path()).expect("open store");
    let original = NewStreamingWindow {
        window_index: 0,
        start_ms: 0,
        end_ms: 1_000,
        text: "Привет, мир.".to_string(),
        language: "ru".to_string(),
        outcome_ok: true,
    };
    store
        .append_window(id, &original, 200)
        .expect("append source");
    set_streaming_translation_enabled(temp.path(), id, true).expect("enable translation");

    let error = translate_and_store_if_current(
        temp.path(),
        id,
        0,
        "en",
        &original.text,
        None,
        1_000,
        |_, _, _| {
            store
                .append_window(
                    id,
                    &NewStreamingWindow {
                        text: "Привет, новый мир.".to_string(),
                        ..original.clone()
                    },
                    900,
                )
                .expect("revise source while inference is in flight");
            Ok("Hello, world.".to_string())
        },
    )
    .expect_err("a translation of obsolete source text must not persist");

    assert!(matches!(error, AppError::Llm(_)));
    assert!(store
        .list_translations(id, "en")
        .expect("list translations")
        .is_empty());
}

#[test]
fn production_translation_rejects_failed_windows_before_inference() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    let store = StreamingStore::open(temp.path()).expect("open store");
    store
        .append_window(
            id,
            &NewStreamingWindow {
                window_index: 0,
                start_ms: 0,
                end_ms: 1_000,
                text: "Unavailable source".to_string(),
                language: "en".to_string(),
                outcome_ok: false,
            },
            200,
        )
        .expect("append failed source");
    set_streaming_translation_enabled(temp.path(), id, true).expect("enable translation");

    let failed_called = std::cell::Cell::new(false);
    let result = translate_and_store_if_current(
        temp.path(),
        id,
        0,
        "ru",
        "Unavailable source",
        None,
        1_000,
        |_, _, _| {
            failed_called.set(true);
            Ok("Недоступно".to_string())
        },
    );
    assert!(result.is_err(), "failed windows are not translatable");
    assert!(
        !failed_called.get(),
        "failed windows must be rejected before consuming inference"
    );
}

#[test]
fn production_translation_rejects_mismatched_windows_before_inference() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    let store = StreamingStore::open(temp.path()).expect("open store");
    store
        .append_window(
            id,
            &NewStreamingWindow {
                window_index: 0,
                start_ms: 0,
                end_ms: 1_000,
                text: "Current source".to_string(),
                language: "en".to_string(),
                outcome_ok: true,
            },
            200,
        )
        .expect("append source");
    set_streaming_translation_enabled(temp.path(), id, true).expect("enable translation");

    let mismatch_called = std::cell::Cell::new(false);
    let result = translate_and_store_if_current(
        temp.path(),
        id,
        0,
        "ru",
        "A different source",
        None,
        1_000,
        |_, _, _| {
            mismatch_called.set(true);
            Ok("Другой источник".to_string())
        },
    );
    assert!(result.is_err(), "stale source text is not translatable");
    assert!(
        !mismatch_called.get(),
        "stale source text must be rejected before consuming inference"
    );
}

// --- WP-93: list_streaming_translations, the read counterpart to
// translate_streaming_window the frontend uses to reuse already-
// persisted translations instead of re-running the model. ---

#[test]
fn list_streaming_translations_returns_empty_when_none_are_stored() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let rows = list_streaming_translations(temp.path(), id, "en").expect("list translations");

    assert!(rows.is_empty());
}

#[test]
fn list_streaming_translations_returns_persisted_rows_with_source_text() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    translate_and_store(
        temp.path(),
        id,
        0,
        "en",
        "Привет, мир.",
        None,
        1_000,
        |_, _, _| Ok("Hello, world.".to_string()),
    )
    .expect("translate and store");

    let rows = list_streaming_translations(temp.path(), id, "en").expect("list translations");

    assert_eq!(
        rows,
        vec![StreamingTranslationDto {
            window_index: 0,
            source_text: "Привет, мир.".to_string(),
            translated_text: "Hello, world.".to_string(),
        }]
    );
}

#[test]
fn list_streaming_translations_orders_by_window_index() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    translate_and_store(
        temp.path(),
        id,
        5,
        "en",
        "Second.",
        None,
        1_000,
        |_, _, _| Ok("Second (en).".to_string()),
    )
    .expect("translate and store paragraph 5");
    translate_and_store(
        temp.path(),
        id,
        0,
        "en",
        "First.",
        None,
        1_000,
        |_, _, _| Ok("First (en).".to_string()),
    )
    .expect("translate and store paragraph 0");

    let rows = list_streaming_translations(temp.path(), id, "en").expect("list translations");

    assert_eq!(
        rows.iter().map(|r| r.window_index).collect::<Vec<_>>(),
        vec![0, 5]
    );
}

#[test]
fn list_streaming_translations_scopes_by_target_language() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    translate_and_store(
        temp.path(),
        id,
        0,
        "en",
        "Привет.",
        None,
        1_000,
        |_, _, _| Ok("Hi.".to_string()),
    )
    .expect("translate en");
    translate_and_store(temp.path(), id, 0, "ru", "Hi.", None, 1_000, |_, _, _| {
        Ok("Привет.".to_string())
    })
    .expect("translate ru");

    let en_rows = list_streaming_translations(temp.path(), id, "en").expect("list en translations");
    let ru_rows = list_streaming_translations(temp.path(), id, "ru").expect("list ru translations");

    assert_eq!(en_rows.len(), 1);
    assert_eq!(en_rows[0].translated_text, "Hi.");
    assert_eq!(ru_rows.len(), 1);
    assert_eq!(ru_rows[0].translated_text, "Привет.");
}

#[test]
fn list_streaming_translations_rejects_an_unsupported_target_language() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let result = list_streaming_translations(temp.path(), id, "fr");

    assert!(matches!(result, Err(AppError::Llm(_))));
}

#[test]
fn list_streaming_translations_rejects_an_unknown_session() {
    let temp = tempfile::tempdir().expect("temp dir");

    let result = list_streaming_translations(temp.path(), 999_999, "en");

    assert!(matches!(result, Err(AppError::Store(_))));
}

#[test]
fn streaming_translation_dto_round_trips() {
    let original = StreamingTranslationDto {
        window_index: 3,
        source_text: "Исходный текст.".to_string(),
        translated_text: "Source text.".to_string(),
    };

    let json = serde_json::to_value(&original).expect("serialize translation DTO");
    let round_tripped: StreamingTranslationDto =
        serde_json::from_value(json).expect("deserialize translation DTO");

    assert_eq!(round_tripped, original);
}

// --- WP-103: paragraph_key -> window_index rename, exercised end-to-end
// through translate_and_store and list_streaming_translations. ---

#[test]
fn translate_and_store_and_list_use_window_index_naming_end_to_end() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let translated = translate_and_store(
        temp.path(),
        id,
        7,
        "en",
        "Привет, мир.",
        None,
        1_000,
        |_text, _lang, _ctx| Ok("Hello, world.".to_string()),
    )
    .expect("translate and store window 7");

    assert_eq!(translated, "Hello, world.");
    let rows = list_streaming_translations(temp.path(), id, "en").expect("list translations");
    assert_eq!(
        rows,
        vec![StreamingTranslationDto {
            window_index: 7,
            source_text: "Привет, мир.".to_string(),
            translated_text: "Hello, world.".to_string(),
        }]
    );
}

// --- WP-101: translation_enabled on the summary/session DTOs, and the
// set_streaming_translation_enabled facade function. ---

#[test]
fn new_session_summary_and_dto_report_translation_enabled_false() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    let summaries = list_streaming_sessions(temp.path()).expect("list");
    assert_eq!(summaries.len(), 1);
    assert!(!summaries[0].translation_enabled);

    let dto = open_streaming_session(temp.path(), id).expect("open");
    assert!(!dto.translation_enabled);
}

#[test]
fn set_streaming_translation_enabled_is_reflected_by_list_and_open() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");

    set_streaming_translation_enabled(temp.path(), id, true).expect("set translation enabled");

    let summaries = list_streaming_sessions(temp.path()).expect("list");
    assert!(summaries[0].translation_enabled);
    let dto = open_streaming_session(temp.path(), id).expect("open");
    assert!(dto.translation_enabled);
}

#[test]
fn set_streaming_translation_enabled_can_be_turned_back_off() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    set_streaming_translation_enabled(temp.path(), id, true).expect("enable");

    set_streaming_translation_enabled(temp.path(), id, false).expect("disable");

    let dto = open_streaming_session(temp.path(), id).expect("open");
    assert!(!dto.translation_enabled);
}

#[test]
fn set_streaming_translation_enabled_rejects_an_unknown_session() {
    let temp = tempfile::tempdir().expect("temp dir");

    let result = set_streaming_translation_enabled(temp.path(), 999_999, true);

    assert!(matches!(result, Err(AppError::Store(_))));
}

#[test]
fn resume_streaming_session_summary_reflects_persisted_translation_enabled() {
    let temp = tempfile::tempdir().expect("temp dir");
    let id = create_streaming_session(temp.path(), 100).expect("create");
    set_streaming_translation_enabled(temp.path(), id, true).expect("enable");
    StreamingStore::open(temp.path())
        .expect("open store")
        .mark_stopped(id, 500)
        .expect("mark stopped");

    let (summary, _next_index) = resume_streaming_session(temp.path(), id, 600).expect("resume");

    assert!(summary.translation_enabled);
}

#[test]
fn streaming_session_dto_round_trips_with_translation_enabled() {
    let original = StreamingSessionDto {
        id: 7,
        title: "Contract session".to_string(),
        created_at_ms: 42,
        updated_at_ms: 100,
        status: "stopped".to_string(),
        windows: vec![],
        mfu: None,
        prettified_text: None,
        translation_enabled: true,
        translation_target_language: "en".to_string(),
        transcription_engine: Some("local".to_string()),
    };

    let json = serde_json::to_value(&original).expect("serialize streaming session DTO");
    let round_tripped: StreamingSessionDto =
        serde_json::from_value(json).expect("deserialize streaming session DTO");

    assert_eq!(round_tripped, original);
}
