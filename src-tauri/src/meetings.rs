//! Testable persistence facade behind the meeting-library Tauri commands.

use crate::error::AppError;
use crate::error::Result;
use crate::store::{Meeting, MeetingId, MeetingNotes, NewMeeting, NewSegment, Store};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeetingSummaryDto {
    pub id: MeetingId,
    pub title: String,
    pub created_at_ms: i64,
    pub duration_ms: Option<i64>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentDto {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeetingDto {
    pub id: MeetingId,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    pub created_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    pub language: String,
    pub status: String,
    pub segments: Vec<SegmentDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<MeetingNotes>,
}

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
    let notes = store.get_notes(id)?;
    to_dto(meeting, segments, notes)
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

    let notes = store.get_notes(id)?;
    to_dto(meeting, Vec::new(), notes)
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
    let notes = store.get_notes(id)?;
    to_dto(meeting, stored, notes)
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
    let notes = store.get_notes(id)?;
    to_dto(meeting, segments, notes)
}

pub fn delete_meeting(app_support_dir: &Path, id: MeetingId) -> Result<()> {
    Store::open(app_support_dir)?.delete_meeting(id)
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

/// A gap this long (ms) between consecutive same-speaker segments reads as a
/// real pause rather than a whisper segmentation artifact, so it starts a new
/// coalesced block instead of merging.
const COALESCE_GAP_TOLERANCE_MS: i64 = 1_000;

/// Merge consecutive `segments` that share the same present `speaker_id` into
/// single display blocks: text joined with a space, spanning the first
/// segment's `start_ms` to the last segment's `end_ms`. A gap larger than
/// `COALESCE_GAP_TOLERANCE_MS` between same-speaker segments starts a new
/// block. Segments with `speaker_id: None` are never merged with each other
/// or a neighboring speaker, so a diarization failure never fabricates false
/// turn continuity. Read-time/display transform only — the caller's
/// underlying stored rows are untouched.
fn coalesce_by_speaker(segments: Vec<SegmentDto>) -> Vec<SegmentDto> {
    let mut coalesced: Vec<SegmentDto> = Vec::with_capacity(segments.len());
    for segment in segments {
        let merged = match (coalesced.last_mut(), segment.speaker_id) {
            (Some(prev), Some(id))
                if prev.speaker_id == Some(id)
                    && segment.start_ms - prev.end_ms <= COALESCE_GAP_TOLERANCE_MS =>
            {
                prev.end_ms = segment.end_ms;
                prev.text.push(' ');
                prev.text.push_str(&segment.text);
                true
            }
            _ => false,
        };
        if !merged {
            coalesced.push(segment);
        }
    }
    coalesced
}

fn to_dto(
    meeting: Meeting,
    segments: Vec<crate::store::StoredSegment>,
    notes: Option<MeetingNotes>,
) -> Result<MeetingDto> {
    let segments = segments
        .into_iter()
        .map(|segment| SegmentDto {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: segment.text,
            speaker_id: segment.speaker_id,
        })
        .collect();
    Ok(MeetingDto {
        id: meeting.id,
        title: meeting.title,
        source_path: meeting.source_path,
        source_name: meeting.source_name,
        created_at_ms: meeting.created_at_ms,
        duration_ms: meeting.duration_ms,
        language: meeting.language,
        status: meeting.status,
        segments: coalesce_by_speaker(segments),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::store::{NewSegment, Store};

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
    fn meeting_dto_round_trips_through_the_ipc_json_contract() {
        let original = MeetingDto {
            id: 7,
            title: "Contract meeting".to_string(),
            source_path: Some("/recordings/contract.m4a".to_string()),
            source_name: Some("contract.m4a".to_string()),
            created_at_ms: 42,
            duration_ms: Some(1_000),
            language: "ru".to_string(),
            status: "finished".to_string(),
            segments: vec![SegmentDto {
                start_ms: 0,
                end_ms: 1_000,
                text: "Saved transcript".to_string(),
                speaker_id: Some(3),
            }],
            notes: None,
        };

        let json = serde_json::to_value(&original).expect("serialize meeting DTO");
        let round_tripped: MeetingDto =
            serde_json::from_value(json).expect("deserialize meeting DTO");

        assert_eq!(round_tripped, original);
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
            vec![SegmentDto {
                start_ms: 0,
                end_ms: 2_000,
                text: "Saved".to_string(),
                speaker_id: Some(1),
            }],
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
                SegmentDto {
                    start_ms: 0,
                    end_ms: 1_000,
                    text: "First".to_string(),
                    speaker_id: Some(1),
                },
                SegmentDto {
                    start_ms: 1_000,
                    end_ms: 3_500,
                    text: "Second".to_string(),
                    speaker_id: None,
                },
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

    fn dto(start_ms: i64, end_ms: i64, text: &str, speaker_id: Option<i64>) -> SegmentDto {
        SegmentDto {
            start_ms,
            end_ms,
            text: text.to_string(),
            speaker_id,
        }
    }

    // EP/BVA: same-speaker merge, gap-tolerance boundary (merge at exactly
    // the tolerance, split just past it), different-speaker split,
    // speakerless never merges, empty/single pass through unchanged.

    #[test]
    fn coalesce_merges_consecutive_segments_sharing_the_same_speaker() {
        let segments = vec![
            dto(0, 1_000, "Hello", Some(1)),
            dto(1_000, 2_000, "there", Some(1)),
            dto(2_000, 3_000, "friend", Some(1)),
        ];

        assert_eq!(
            coalesce_by_speaker(segments),
            vec![dto(0, 3_000, "Hello there friend", Some(1))]
        );
    }

    #[test]
    fn coalesce_merges_when_the_gap_is_exactly_at_the_tolerance_boundary() {
        let segments = vec![
            dto(0, 1_000, "First", Some(1)),
            dto(1_000 + COALESCE_GAP_TOLERANCE_MS, 3_000, "Second", Some(1)),
        ];

        assert_eq!(
            coalesce_by_speaker(segments),
            vec![dto(0, 3_000, "First Second", Some(1))]
        );
    }

    #[test]
    fn coalesce_starts_a_new_block_when_the_gap_exceeds_the_tolerance() {
        let second_start = 1_000 + COALESCE_GAP_TOLERANCE_MS + 1;
        let segments = vec![
            dto(0, 1_000, "First", Some(1)),
            dto(second_start, 3_000, "Second", Some(1)),
        ];

        assert_eq!(coalesce_by_speaker(segments.clone()), segments);
    }

    #[test]
    fn coalesce_never_merges_across_different_speakers() {
        let segments = vec![
            dto(0, 1_000, "First", Some(1)),
            dto(1_000, 2_000, "Second", Some(2)),
        ];

        assert_eq!(coalesce_by_speaker(segments.clone()), segments);
    }

    #[test]
    fn coalesce_never_merges_speakerless_segments_with_each_other_or_a_neighbor() {
        let segments = vec![
            dto(0, 1_000, "A", None),
            dto(1_000, 2_000, "B", None),
            dto(2_000, 3_000, "C", Some(1)),
        ];

        assert_eq!(coalesce_by_speaker(segments.clone()), segments);
    }

    #[test]
    fn coalesce_passes_through_an_empty_list_and_a_single_segment_without_panicking() {
        assert_eq!(coalesce_by_speaker(Vec::new()), Vec::<SegmentDto>::new());

        let single = vec![dto(0, 1_000, "Solo", Some(1))];
        assert_eq!(coalesce_by_speaker(single.clone()), single);
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
            vec![SegmentDto {
                start_ms: 0,
                end_ms: 1_400,
                text: "Uh, your presenter now.".to_string(),
                speaker_id: None,
            }],
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
            vec![SegmentDto {
                start_ms: 0,
                end_ms: 1_000,
                text: "Okay, let's start from today.".to_string(),
                speaker_id: None,
            }],
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
}
