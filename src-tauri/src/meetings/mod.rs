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
        title: "New Transcription".to_string(),
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
        .ok_or_else(|| AppError::Store(format!("transcription {id} was not found")))?;
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
        .ok_or_else(|| AppError::Store(format!("transcription {id} was not found")))?;

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
        .ok_or_else(|| AppError::Store(format!("transcription {id} was not found")))?;

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
    meeting.duration_ms = duration_ms;
    meeting.language = language;
    meeting.status = "finished".to_string();
    store.complete_transcript(&meeting, &rows)?;

    let stored = store.list_segments(id)?;
    let mfu = store.get_mfu(id)?;
    to_dto(meeting, stored, mfu)
}

pub fn rename_meeting(app_support_dir: &Path, id: MeetingId, title: String) -> Result<MeetingDto> {
    let title = validate_title(title)?;
    let store = Store::open(app_support_dir)?;
    let mut meeting = store
        .get_meeting(id)?
        .ok_or_else(|| AppError::Store(format!("transcription {id} was not found")))?;
    meeting.title = title;
    store.update_meeting(&meeting)?;
    let segments = store.list_segments(id)?;
    let mfu = store.get_mfu(id)?;
    to_dto(meeting, segments, mfu)
}

pub fn delete_meeting(app_support_dir: &Path, id: MeetingId) -> Result<()> {
    Store::open(app_support_dir)?.delete_meeting(id)
}

pub fn clear_meeting(app_support_dir: &Path, id: MeetingId) -> Result<MeetingDto> {
    let store = Store::open(app_support_dir)?;
    store.clear_meeting_content(id, crate::transcribe::UNDETECTED_LANGUAGE)?;
    let meeting = store
        .get_meeting(id)?
        .ok_or_else(|| AppError::Store(format!("transcription {id} was not found")))?;
    to_dto(meeting, Vec::new(), None)
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
        .ok_or_else(|| AppError::Store(format!("transcription {id} was not found")))?;

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
    let meeting = store.get_meeting(mfu.meeting_id)?.ok_or_else(|| {
        AppError::Store(format!("transcription {} was not found", mfu.meeting_id))
    })?;
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
        .ok_or_else(|| AppError::Store(format!("transcription {id} was not found")))?;

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
        return Err(AppError::Store("transcription title is required".into()));
    }
    if title.chars().count() > 120 {
        return Err(AppError::Store(
            "transcription title must be 120 characters or fewer".into(),
        ));
    }
    Ok(title)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
