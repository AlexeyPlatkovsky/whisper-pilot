//! Testable persistence facade behind the meeting-library Tauri commands.

pub(crate) mod dto;

use crate::diarize::SpeakerTurn;
use crate::error::{AppError, Result};
use crate::store::{MeetingId, MeetingMfu, NewMeeting, NewSegment, Store};
use std::path::Path;

pub(crate) use dto::{coalesce_by_speaker, to_dto};
pub use dto::{MeetingDto, MeetingSummaryDto, SegmentDto};

pub fn create_empty_meeting(app_support_dir: &Path, created_at_ms: i64) -> Result<MeetingDto> {
    let store = Store::open(app_support_dir)?;
    let meeting = store.create_meeting(NewMeeting {
        title: "New Meeting".to_string(),
        source_path: None,
        source_name: None,
        created_at_ms,
        duration_ms: None,
        // Nothing has been decoded yet, so the meeting claims no language.
        // `save_transcript` fills this in with what whisper detected.
        language: crate::transcribe::UNDETECTED_LANGUAGE.to_string(),
        status: "no_files".to_string(),
    })?;
    to_dto(meeting, Vec::new(), None)
}

pub fn list_meetings(app_support_dir: &Path) -> Result<Vec<MeetingSummaryDto>> {
    let summaries = Store::open(app_support_dir)?
        .list_meetings()?
        .into_iter()
        .map(|summary| MeetingSummaryDto {
            id: summary.id,
            title: summary.title,
            created_at_ms: summary.created_at_ms,
            duration_ms: summary.duration_ms,
            status: summary.status,
        })
        .collect();
    Ok(summaries)
}

pub fn open_meeting(app_support_dir: &Path, id: MeetingId) -> Result<MeetingDto> {
    let store = Store::open(app_support_dir)?;
    let meeting = store
        .get_meeting(id)?
        .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;
    let segments = store.list_segments(id)?;
    let mfu = store.get_mfu(id)?;
    to_dto(meeting, segments, mfu)
}

/// Attach a source file to a meeting, or clear it when `source_path` is `None`.
/// Either way any prior transcript is discarded: attaching a file marks the
/// meeting `ready` to transcribe, clearing it returns it to the empty
/// `no_files` state. Selecting the file and running transcription are separate,
/// explicit actions.
pub fn set_meeting_source(
    app_support_dir: &Path,
    id: MeetingId,
    source_path: Option<String>,
) -> Result<MeetingDto> {
    let store = Store::open(app_support_dir)?;
    let mut meeting = store
        .get_meeting(id)?
        .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;

    // Changing the source invalidates the existing transcript and duration.
    store.replace_segments(id, &[])?;
    match source_path {
        Some(path) => {
            let name = Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            meeting.source_path = Some(path);
            meeting.source_name = Some(name);
            meeting.status = "ready".to_string();
        }
        None => {
            meeting.source_path = None;
            meeting.source_name = None;
            meeting.status = "no_files".to_string();
        }
    }
    meeting.duration_ms = None;
    // The detected language described the transcript that just went away, so it
    // is cleared alongside the duration rather than left describing nothing.
    meeting.language = crate::transcribe::UNDETECTED_LANGUAGE.to_string();
    store.update_meeting(&meeting)?;

    let mfu = store.get_mfu(id)?;
    to_dto(meeting, Vec::new(), mfu)
}

/// Persist a freshly produced transcript against a meeting, marking it
/// `finished`. Segment ordinals follow the supplied order.
///
/// `language` is the code whisper detected while producing this transcript.
/// The stored value is an output of the decode, never an input to it, so
/// whatever the row held before — including a `"ru"` left by the old hardcoded
/// default — is simply replaced.
pub fn save_transcript(
    app_support_dir: &Path,
    id: MeetingId,
    segments: Vec<SegmentDto>,
    duration_ms: Option<i64>,
    language: String,
) -> Result<MeetingDto> {
    let store = Store::open(app_support_dir)?;
    let mut meeting = store
        .get_meeting(id)?
        .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;

    let rows: Vec<NewSegment> = segments
        .iter()
        .enumerate()
        .map(|(ordinal, segment)| NewSegment {
            ordinal: ordinal as i64,
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: segment.text.clone(),
            speaker_id: segment.speaker_id,
        })
        .collect();
    store.replace_segments(id, &rows)?;

    meeting.duration_ms = duration_ms;
    meeting.language = language;
    meeting.status = "finished".to_string();
    store.update_meeting(&meeting)?;

    let stored = store.list_segments(id)?;
    let mfu = store.get_mfu(id)?;
    to_dto(meeting, stored, mfu)
}

pub fn rename_meeting(app_support_dir: &Path, id: MeetingId, title: String) -> Result<MeetingDto> {
    let title = validate_title(title)?;
    let store = Store::open(app_support_dir)?;
    let mut meeting = store
        .get_meeting(id)?
        .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;
    meeting.title = title;
    store.update_meeting(&meeting)?;
    let segments = store.list_segments(id)?;
    let mfu = store.get_mfu(id)?;
    to_dto(meeting, segments, mfu)
}

pub fn delete_meeting(app_support_dir: &Path, id: MeetingId) -> Result<()> {
    Store::open(app_support_dir)?.delete_meeting(id)
}

/// Auto-save an edited segment's text. `index` addresses the meeting's
/// currently displayed (speaker-coalesced) segment list — the same list
/// `open_meeting`/`to_dto` return — not raw storage ordinals. Persisting
/// therefore rewrites storage to match the coalesced view: consecutive
/// same-speaker raw segments the user sees (and edits) as one block become
/// one stored row from this point on. No explicit save action is required.
pub fn update_segment(
    app_support_dir: &Path,
    id: MeetingId,
    index: usize,
    text: String,
) -> Result<MeetingDto> {
    let store = Store::open(app_support_dir)?;
    let meeting = store
        .get_meeting(id)?
        .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;

    let stored = store.list_segments(id)?;
    let mut coalesced = coalesce_by_speaker(
        stored
            .iter()
            .map(|segment| SegmentDto {
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                text: segment.text.clone(),
                speaker_id: segment.speaker_id,
            })
            .collect(),
    );
    let target = coalesced
        .get_mut(index)
        .ok_or_else(|| AppError::Store(format!("segment {index} was not found")))?;
    target.text = text;

    let rows: Vec<NewSegment> = coalesced
        .iter()
        .enumerate()
        .map(|(ordinal, segment)| NewSegment {
            ordinal: ordinal as i64,
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: segment.text.clone(),
            speaker_id: segment.speaker_id,
        })
        .collect();
    store.replace_segments(id, &rows)?;

    let saved = store.list_segments(id)?;
    let mfu = store.get_mfu(id)?;
    to_dto(meeting, saved, mfu)
}

/// Auto-save the meeting mfu fields as the user edits them.
pub fn update_mfu(app_support_dir: &Path, mfu: MeetingMfu) -> Result<MeetingDto> {
    let store = Store::open(app_support_dir)?;
    let meeting = store
        .get_meeting(mfu.meeting_id)?
        .ok_or_else(|| AppError::Store(format!("meeting {} was not found", mfu.meeting_id)))?;
    store.upsert_mfu(&mfu)?;
    let segments = store.list_segments(mfu.meeting_id)?;
    let saved_mfu = store.get_mfu(mfu.meeting_id)?;
    to_dto(meeting, segments, saved_mfu)
}

/// Assign `turns` onto the meeting's already-persisted segments (by their
/// stored `start_ms`/`end_ms` spans) and save the result — the "Diarize"
/// action re-running speaker identification alone, without a Transcribe run.
/// Segments that were never diarized keep whisper's original per-utterance
/// granularity, so a first Diarize run assigns at full precision; a meeting
/// that was already diarized (and therefore coalesced into per-speaker
/// blocks — see `to_dto`) is re-assigned at that coarser, already-collapsed
/// granularity instead.
pub fn diarize_meeting_segments(
    app_support_dir: &Path,
    id: MeetingId,
    turns: &[SpeakerTurn],
) -> Result<MeetingDto> {
    let store = Store::open(app_support_dir)?;
    let meeting = store
        .get_meeting(id)?
        .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;

    let stored = store.list_segments(id)?;
    let mut segments: Vec<crate::transcribe::Segment> = stored
        .iter()
        .map(|segment| crate::transcribe::Segment {
            start_ms: segment.start_ms.max(0) as u64,
            end_ms: segment.end_ms.max(0) as u64,
            text: segment.text.clone(),
            speaker_id: segment.speaker_id.map(|id| id as i32),
        })
        .collect();
    crate::diarize::assign_speaker_ids(&mut segments, turns);

    let rows: Vec<NewSegment> = segments
        .iter()
        .enumerate()
        .map(|(ordinal, segment)| NewSegment {
            ordinal: ordinal as i64,
            start_ms: segment.start_ms as i64,
            end_ms: segment.end_ms as i64,
            text: segment.text.clone(),
            speaker_id: segment.speaker_id.map(i64::from),
        })
        .collect();
    store.replace_segments(id, &rows)?;

    let saved = store.list_segments(id)?;
    let mfu = store.get_mfu(id)?;
    to_dto(meeting, saved, mfu)
}

fn validate_title(title: String) -> Result<String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::Store("meeting title is required".into()));
    }
    if title.chars().count() > 120 {
        return Err(AppError::Store(
            "meeting title must be 120 characters or fewer".into(),
        ));
    }
    Ok(title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::store::{NewMeeting, Store};

    fn meeting(title: &str, created_at_ms: i64) -> NewMeeting {
        NewMeeting {
            title: title.to_string(),
            source_path: Some(format!("/recordings/{title}.m4a")),
            source_name: Some(format!("{title}.m4a")),
            created_at_ms,
            duration_ms: Some(1_000),
            language: "ru".to_string(),
            status: "finished".to_string(),
        }
    }

    fn dto(start_ms: i64, end_ms: i64, text: &str, speaker_id: Option<i64>) -> SegmentDto {
        SegmentDto {
            start_ms,
            end_ms,
            text: text.to_string(),
            speaker_id,
        }
    }

    #[test]
    fn given_saved_meetings_when_listed_and_opened_then_newest_summary_and_full_record_return() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = Store::open(temp.path()).expect("open store");
        let older = store
            .create_meeting(meeting("Older", 1))
            .expect("create older");
        let newest = store
            .create_meeting(meeting("Newest", 2))
            .expect("create newest");
        store
            .replace_segments(
                newest.id,
                &[NewSegment {
                    ordinal: 0,
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "Saved transcript".to_string(),
                    speaker_id: Some(2),
                }],
            )
            .expect("save segment");

        let summaries = list_meetings(temp.path()).expect("list meetings");
        let opened = open_meeting(temp.path(), newest.id).expect("open newest");

        assert_eq!(
            summaries
                .into_iter()
                .map(|summary| summary.id)
                .collect::<Vec<_>>(),
            vec![newest.id, older.id]
        );
        assert_eq!(opened.title, "Newest");
        assert_eq!(opened.source_name.as_deref(), Some("Newest.m4a"));
        assert_eq!(opened.segments[0].text, "Saved transcript");
    }

    #[test]
    fn given_empty_library_when_creating_then_new_meeting_has_no_files_and_no_segments() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");

        let created = create_empty_meeting(temp.path(), 123).expect("create meeting");

        assert_eq!(created.title, "New Meeting");
        assert_eq!(created.status, "no_files");
        assert_eq!(created.source_path, None);
        assert!(created.segments.is_empty());
    }

    #[test]
    fn given_a_new_meeting_when_created_then_its_language_is_undetected_not_forced_russian() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");

        let created = create_empty_meeting(temp.path(), 123).expect("create meeting");

        // Nothing has been decoded yet, so the row must not claim a language.
        // Stamping "ru" here is what forced whisper into Russian on English
        // audio and produced a transcript of hallucinated subtitle credits.
        // Asserted as a literal, not via the production constant, so changing
        // that constant cannot silently move this contract.
        assert_eq!(created.language, "auto");
    }

    #[test]
    fn given_unknown_id_when_opening_then_typed_store_error_is_returned() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");

        let error = open_meeting(temp.path(), 999).expect_err("unknown meeting must fail");

        assert!(matches!(error, AppError::Store(_)));
    }

    #[test]
    fn given_empty_meeting_when_source_attached_then_it_is_ready_with_a_derived_name() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");

        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");
        let attached = set_meeting_source(
            temp.path(),
            created.id,
            Some("/recordings/Weekly sync.m4a".to_string()),
        )
        .expect("attach source");

        assert_eq!(attached.status, "ready");
        assert_eq!(
            attached.source_path.as_deref(),
            Some("/recordings/Weekly sync.m4a")
        );
        assert_eq!(attached.source_name.as_deref(), Some("Weekly sync.m4a"));
        assert!(attached.segments.is_empty());
    }

    #[test]
    fn given_transcribed_meeting_when_source_replaced_or_cleared_then_transcript_is_discarded() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");
        set_meeting_source(temp.path(), created.id, Some("/a.m4a".to_string()))
            .expect("attach source");
        save_transcript(
            temp.path(),
            created.id,
            vec![dto(0, 2_000, "Saved", Some(1))],
            Some(2_000),
            "en".to_string(),
        )
        .expect("save transcript");

        // Attaching a different file drops the prior transcript and duration.
        let replaced = set_meeting_source(temp.path(), created.id, Some("/b.m4a".to_string()))
            .expect("replace source");
        assert_eq!(replaced.status, "ready");
        assert_eq!(replaced.source_name.as_deref(), Some("b.m4a"));
        assert!(replaced.segments.is_empty());
        assert_eq!(replaced.duration_ms, None);

        // The detected language went with the discarded transcript.
        assert_eq!(replaced.language, "auto");

        // Clearing the file returns the meeting to the empty state.
        let cleared = set_meeting_source(temp.path(), created.id, None).expect("clear source");
        assert_eq!(cleared.status, "no_files");
        assert_eq!(cleared.source_path, None);
        assert_eq!(cleared.source_name, None);
        assert!(cleared.segments.is_empty());
    }

    #[test]
    fn given_attached_meeting_when_transcript_saved_then_it_is_finished_and_reopens_with_segments()
    {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");
        set_meeting_source(temp.path(), created.id, Some("/talk.m4a".to_string()))
            .expect("attach source");

        let saved = save_transcript(
            temp.path(),
            created.id,
            vec![
                dto(0, 1_000, "First", Some(1)),
                dto(1_000, 3_500, "Second", None),
            ],
            Some(3_500),
            "en".to_string(),
        )
        .expect("save transcript");

        assert_eq!(saved.status, "finished");
        assert_eq!(saved.duration_ms, Some(3_500));
        let reopened = open_meeting(temp.path(), created.id).expect("reopen meeting");
        assert_eq!(
            reopened
                .segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>(),
            vec!["First", "Second"]
        );
        assert_eq!(reopened.status, "finished");
    }

    #[test]
    fn given_consecutive_same_speaker_segments_when_saved_then_display_coalesces_but_stored_rows_stay_fine_grained(
    ) {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");
        set_meeting_source(temp.path(), created.id, Some("/talk.m4a".to_string()))
            .expect("attach source");

        let saved = save_transcript(
            temp.path(),
            created.id,
            vec![
                dto(0, 1_000, "Hello", Some(1)),
                dto(1_000, 2_000, "there", Some(1)),
            ],
            Some(2_000),
            "en".to_string(),
        )
        .expect("save transcript");

        assert_eq!(saved.segments, vec![dto(0, 2_000, "Hello there", Some(1))]);

        let stored = Store::open(temp.path())
            .expect("open store")
            .list_segments(created.id)
            .expect("list stored segments");
        assert_eq!(stored.len(), 2, "underlying rows must stay fine-grained");
        assert_eq!(stored[0].text, "Hello");
        assert_eq!(stored[1].text, "there");

        let reopened = open_meeting(temp.path(), created.id).expect("reopen meeting");
        assert_eq!(
            reopened.segments,
            vec![dto(0, 2_000, "Hello there", Some(1))]
        );
    }

    #[test]
    fn given_a_completed_transcription_when_saved_then_the_detected_language_is_persisted() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");
        set_meeting_source(temp.path(), created.id, Some("/standup.mp4".to_string()))
            .expect("attach source");

        let saved = save_transcript(
            temp.path(),
            created.id,
            vec![dto(0, 1_400, "Uh, your presenter now.", None)],
            Some(1_400),
            "en".to_string(),
        )
        .expect("save transcript");

        assert_eq!(saved.language, "en");
        assert_eq!(
            open_meeting(temp.path(), created.id)
                .expect("reopen meeting")
                .language,
            "en"
        );
    }

    #[test]
    fn given_a_legacy_russian_row_when_re_transcribed_then_the_detected_language_replaces_it() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = Store::open(temp.path()).expect("open store");
        // A row stamped "ru" by the old hardcoded default, never a user choice.
        let legacy = store
            .create_meeting(meeting("Legacy", 1))
            .expect("create legacy meeting");
        assert_eq!(legacy.language, "ru");

        let saved = save_transcript(
            temp.path(),
            legacy.id,
            vec![dto(0, 1_000, "Okay, let's start from today.", None)],
            Some(1_000),
            "en".to_string(),
        )
        .expect("save transcript");

        // The stored value is an output, never fed back in as a decode input,
        // so no migration is needed to unstick a legacy row.
        assert_eq!(saved.language, "en");
    }

    #[test]
    fn given_unknown_id_when_setting_source_or_saving_transcript_then_typed_error_returns() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");

        assert!(matches!(
            set_meeting_source(temp.path(), 999, Some("/x.m4a".to_string())),
            Err(AppError::Store(_))
        ));
        assert!(matches!(
            save_transcript(temp.path(), 999, Vec::new(), None, "en".to_string()),
            Err(AppError::Store(_))
        ));
    }

    #[test]
    fn given_saved_meeting_when_renamed_then_trimmed_title_is_persisted() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = Store::open(temp.path()).expect("open store");
        let saved = store
            .create_meeting(meeting("Original", 1))
            .expect("create meeting");

        let renamed = rename_meeting(temp.path(), saved.id, "  Roadmap review  ".to_string())
            .expect("rename meeting");

        assert_eq!(renamed.title, "Roadmap review");
        assert_eq!(
            open_meeting(temp.path(), saved.id)
                .expect("open renamed meeting")
                .title,
            "Roadmap review"
        );
    }

    #[test]
    fn ep_bva_rename_rejects_blank_overlong_and_unknown_meetings() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = Store::open(temp.path()).expect("open store");
        let saved = store
            .create_meeting(meeting("Original", 1))
            .expect("create meeting");

        for title in ["   ".to_string(), "a".repeat(121)] {
            assert!(matches!(
                rename_meeting(temp.path(), saved.id, title),
                Err(AppError::Store(_))
            ));
        }
        assert!(matches!(
            rename_meeting(temp.path(), 999, "Valid title".to_string()),
            Err(AppError::Store(_))
        ));
        assert_eq!(
            open_meeting(temp.path(), saved.id)
                .expect("open unchanged meeting")
                .title,
            "Original"
        );
    }

    #[test]
    fn given_saved_meeting_when_deleted_then_it_cannot_be_opened_or_deleted_again() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let store = Store::open(temp.path()).expect("open store");
        let saved = store
            .create_meeting(meeting("Disposable", 1))
            .expect("create meeting");

        delete_meeting(temp.path(), saved.id).expect("delete meeting");

        assert!(matches!(
            open_meeting(temp.path(), saved.id),
            Err(AppError::Store(_))
        ));
        assert!(matches!(
            delete_meeting(temp.path(), saved.id),
            Err(AppError::Store(_))
        ));
    }

    #[test]
    fn given_saved_transcript_when_a_segment_is_edited_then_the_new_text_persists_and_reopens() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");
        set_meeting_source(temp.path(), created.id, Some("/talk.m4a".to_string()))
            .expect("attach source");
        save_transcript(
            temp.path(),
            created.id,
            vec![
                dto(0, 1_000, "Hello", Some(1)),
                dto(2_000, 3_000, "World", Some(2)),
            ],
            Some(3_000),
            "en".to_string(),
        )
        .expect("save transcript");

        let updated = update_segment(temp.path(), created.id, 0, "Hi there".to_string())
            .expect("update segment");

        assert_eq!(updated.segments[0].text, "Hi there");
        assert_eq!(updated.segments[1].text, "World");

        let reopened = open_meeting(temp.path(), created.id).expect("reopen meeting");
        assert_eq!(reopened.segments[0].text, "Hi there");
    }

    #[test]
    fn given_coalesced_segments_when_the_merged_block_is_edited_then_storage_collapses_to_match_the_display(
    ) {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");
        set_meeting_source(temp.path(), created.id, Some("/talk.m4a".to_string()))
            .expect("attach source");
        save_transcript(
            temp.path(),
            created.id,
            vec![
                dto(0, 1_000, "Hello", Some(1)),
                dto(1_000, 2_000, "there", Some(1)),
            ],
            Some(2_000),
            "en".to_string(),
        )
        .expect("save transcript");

        let updated = update_segment(temp.path(), created.id, 0, "Hi all".to_string())
            .expect("update segment");
        assert_eq!(updated.segments, vec![dto(0, 2_000, "Hi all", Some(1))]);

        let stored = Store::open(temp.path())
            .expect("open store")
            .list_segments(created.id)
            .expect("list stored segments");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].text, "Hi all");
    }

    #[test]
    fn given_an_out_of_range_index_or_unknown_meeting_when_a_segment_is_edited_then_a_typed_error_returns(
    ) {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");
        set_meeting_source(temp.path(), created.id, Some("/talk.m4a".to_string()))
            .expect("attach source");
        save_transcript(
            temp.path(),
            created.id,
            vec![dto(0, 1_000, "Hello", Some(1))],
            Some(1_000),
            "en".to_string(),
        )
        .expect("save transcript");

        assert!(matches!(
            update_segment(temp.path(), created.id, 5, "x".to_string()),
            Err(AppError::Store(_))
        ));
        assert!(matches!(
            update_segment(temp.path(), 999_999, 0, "x".to_string()),
            Err(AppError::Store(_))
        ));
    }

    #[test]
    fn given_a_meeting_when_notes_are_edited_then_they_persist_and_reopen() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");

        let mfu = MeetingMfu {
            meeting_id: created.id,
            summary: "Summary".to_string(),
            decisions: "Decided X".to_string(),
            action_items: "Do Y".to_string(),
            open_questions: "".to_string(),
            participants: "Alice, Bob".to_string(),
        };
        let updated = update_mfu(temp.path(), mfu.clone()).expect("update mfu");
        assert_eq!(updated.mfu, Some(mfu.clone()));

        let reopened = open_meeting(temp.path(), created.id).expect("reopen meeting");
        assert_eq!(reopened.mfu, Some(mfu));
    }

    #[test]
    fn given_an_unknown_meeting_when_notes_are_edited_then_a_typed_error_returns() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let mfu = MeetingMfu {
            meeting_id: 999_999,
            summary: String::new(),
            decisions: String::new(),
            action_items: String::new(),
            open_questions: String::new(),
            participants: String::new(),
        };
        assert!(matches!(
            update_mfu(temp.path(), mfu),
            Err(AppError::Store(_))
        ));
    }

    #[test]
    fn given_no_source_when_opened_then_source_missing_is_false() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");

        assert!(!created.source_missing);
        let reopened = open_meeting(temp.path(), created.id).expect("reopen meeting");
        assert!(!reopened.source_missing);
    }

    #[test]
    fn given_an_existing_source_file_when_opened_then_source_missing_is_false() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let source = temp.path().join("talk.m4a");
        std::fs::write(&source, b"fake audio").expect("write fake source file");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");

        let attached = set_meeting_source(
            temp.path(),
            created.id,
            Some(source.to_string_lossy().to_string()),
        )
        .expect("attach source");

        assert!(!attached.source_missing);
    }

    #[test]
    fn given_a_source_file_removed_after_attaching_when_reopened_then_source_missing_is_true() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let source = temp.path().join("talk.m4a");
        std::fs::write(&source, b"fake audio").expect("write fake source file");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");
        set_meeting_source(
            temp.path(),
            created.id,
            Some(source.to_string_lossy().to_string()),
        )
        .expect("attach source");

        std::fs::remove_file(&source).expect("remove source file");

        let reopened = open_meeting(temp.path(), created.id).expect("reopen meeting");
        assert!(reopened.source_missing);
        // Transcript/mfu access is unaffected — this only gates re-transcribing.
        assert_eq!(reopened.status, "ready");
    }

    #[test]
    fn given_an_undiarized_transcript_when_diarized_then_speaker_ids_are_assigned_and_text_is_unchanged(
    ) {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");
        set_meeting_source(temp.path(), created.id, Some("/talk.m4a".to_string()))
            .expect("attach source");
        save_transcript(
            temp.path(),
            created.id,
            vec![
                dto(0, 1_000, "Hello", None),
                dto(1_000, 2_000, "World", None),
            ],
            Some(2_000),
            "en".to_string(),
        )
        .expect("save transcript");

        let turns = [
            SpeakerTurn {
                start_ms: 0,
                end_ms: 1_000,
                speaker: 1,
            },
            SpeakerTurn {
                start_ms: 1_000,
                end_ms: 2_000,
                speaker: 2,
            },
        ];
        let diarized =
            diarize_meeting_segments(temp.path(), created.id, &turns).expect("diarize meeting");

        assert_eq!(diarized.segments[0].text, "Hello");
        assert_eq!(diarized.segments[0].speaker_id, Some(1));
        assert_eq!(diarized.segments[1].text, "World");
        assert_eq!(diarized.segments[1].speaker_id, Some(2));

        let reopened = open_meeting(temp.path(), created.id).expect("reopen meeting");
        assert_eq!(reopened.segments, diarized.segments);
    }

    #[test]
    fn given_an_unknown_meeting_when_diarized_then_a_typed_error_returns() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");

        assert!(matches!(
            diarize_meeting_segments(temp.path(), 999_999, &[]),
            Err(AppError::Store(_))
        ));
    }

    #[test]
    fn given_empty_turns_when_diarized_then_segments_are_left_speakerless() {
        let temp = tempfile::tempdir().expect("temporary app-support directory");
        let created = create_empty_meeting(temp.path(), 1).expect("create meeting");
        set_meeting_source(temp.path(), created.id, Some("/talk.m4a".to_string()))
            .expect("attach source");
        save_transcript(
            temp.path(),
            created.id,
            vec![dto(0, 1_000, "Hello", None)],
            Some(1_000),
            "en".to_string(),
        )
        .expect("save transcript");

        let diarized =
            diarize_meeting_segments(temp.path(), created.id, &[]).expect("diarize meeting");

        assert_eq!(diarized.segments[0].speaker_id, None);
    }
}
