//! Testable persistence facade behind the meeting-library Tauri commands.

use crate::error::AppError;
use crate::error::Result;
use crate::store::{Meeting, MeetingId, MeetingNotes, NewMeeting, Store};
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
        language: "ru".to_string(),
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

fn to_dto(
    meeting: Meeting,
    segments: Vec<crate::store::StoredSegment>,
    notes: Option<MeetingNotes>,
) -> Result<MeetingDto> {
    Ok(MeetingDto {
        id: meeting.id,
        title: meeting.title,
        source_path: meeting.source_path,
        source_name: meeting.source_name,
        created_at_ms: meeting.created_at_ms,
        duration_ms: meeting.duration_ms,
        language: meeting.language,
        status: meeting.status,
        segments: segments
            .into_iter()
            .map(|segment| SegmentDto {
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                text: segment.text,
                speaker_id: segment.speaker_id,
            })
            .collect(),
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
