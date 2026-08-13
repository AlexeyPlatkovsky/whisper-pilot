//! Meeting DTOs and display transforms: the IPC wire types and the read-time
//! speaker-coalescing transform that turns fine-grained stored rows into the
//! per-speaker display blocks the workspace renders.

use crate::store::{Meeting, MeetingId, MeetingMfu};
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
    pub mfu: Option<MeetingMfu>,
    /// `true` when this meeting has an attached source file that is no longer
    /// readable at `source_path` (moved or deleted since it was attached).
    /// `false` for a meeting with no source at all. The transcript and MFU
    /// stay readable/editable either way; only re-transcribing needs the file.
    pub source_missing: bool,
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
pub(crate) fn coalesce_by_speaker(segments: Vec<SegmentDto>) -> Vec<SegmentDto> {
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

pub(crate) fn to_dto(
    meeting: Meeting,
    segments: Vec<crate::store::StoredSegment>,
    mfu: Option<MeetingMfu>,
) -> crate::error::Result<MeetingDto> {
    let segments = segments
        .into_iter()
        .map(|segment| SegmentDto {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: segment.text,
            speaker_id: segment.speaker_id,
        })
        .collect();
    let source_missing = meeting
        .source_path
        .as_ref()
        .is_some_and(|path| !Path::new(path).exists());
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
        mfu,
        source_missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn dto(start_ms: i64, end_ms: i64, text: &str, speaker_id: Option<i64>) -> SegmentDto {
        SegmentDto {
            start_ms,
            end_ms,
            text: text.to_string(),
            speaker_id,
        }
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
            segments: vec![dto(0, 1_000, "Saved transcript", Some(3))],
            mfu: None,
            source_missing: false,
        };

        let json = serde_json::to_value(&original).expect("serialize meeting DTO");
        let round_tripped: MeetingDto =
            serde_json::from_value(json).expect("deserialize meeting DTO");

        assert_eq!(round_tripped, original);
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
    fn to_dto_marks_source_missing_when_an_attached_source_file_is_gone() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("talk.m4a");
        std::fs::write(&source, b"fake audio").unwrap();
        let store = Store::open(temp.path()).unwrap();
        let created = store
            .create_meeting(crate::store::NewMeeting {
                title: "T".to_string(),
                source_path: Some(source.to_string_lossy().to_string()),
                source_name: Some("talk.m4a".to_string()),
                created_at_ms: 0,
                duration_ms: Some(1_000),
                language: "en".to_string(),
                status: "ready".to_string(),
            })
            .unwrap();

        let present = to_dto(
            store.get_meeting(created.id).unwrap().unwrap(),
            Vec::new(),
            None,
        )
        .unwrap();
        assert!(!present.source_missing);

        std::fs::remove_file(&source).unwrap();
        let gone = to_dto(
            store.get_meeting(created.id).unwrap().unwrap(),
            Vec::new(),
            None,
        )
        .unwrap();
        assert!(gone.source_missing);
    }
}
