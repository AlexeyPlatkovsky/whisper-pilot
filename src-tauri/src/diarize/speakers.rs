//! Speaker turn merging and segment assignment: overlap-based speaker
//! resolution (WP-3/WP-7) and applying a diarization outcome to segments.

use crate::error::Result;
use crate::transcribe;
use std::collections::BTreeMap;

/// One speaker's contiguous span of speech, in milliseconds.
///
/// Serializable because WP-53 runs the engine in a child process and the turns
/// come back as a JSON payload; this is not part of the IPC contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpeakerTurn {
    pub start_ms: u32,
    pub end_ms: u32,
    pub speaker: i32,
}

/// Overlap duration (ms) between two `[start, end)` spans, or 0 if disjoint.
/// Callers must pass well-formed spans (`start <= end`), matching the
/// invariant `Segment`/`SpeakerTurn` already uphold; a malformed span is a
/// caller bug, not a value this function tries to recover from.
fn overlap_ms(a: (u32, u32), b: (u32, u32)) -> u32 {
    debug_assert!(
        a.0 <= a.1 && b.0 <= b.1,
        "spans must be well-formed (start <= end)"
    );
    let lo = a.0.max(b.0);
    let hi = a.1.min(b.1);
    hi.saturating_sub(lo)
}

/// Temporal gap (ms) between two `[start, end)` spans, or 0 if they touch or
/// overlap. Same well-formed-span precondition as `overlap_ms`.
fn gap_ms(a: (u32, u32), b: (u32, u32)) -> u32 {
    debug_assert!(
        a.0 <= a.1 && b.0 <= b.1,
        "spans must be well-formed (start <= end)"
    );
    let lo = a.0.max(b.0);
    let hi = a.1.min(b.1);
    lo.saturating_sub(hi)
}

/// Assign each `segments` span the speaker whose diarization `turns`
/// maximally overlap it in time. Ties (including all-zero overlap) are
/// broken by the lowest speaker id. A segment with zero overlap against
/// every turn falls back to the temporally nearest turn's speaker, same
/// tie-break. An empty `turns` list yields `None` for every segment.
pub fn merge_segments_with_turns(
    segments: &[(u32, u32)],
    turns: &[SpeakerTurn],
) -> Vec<Option<i32>> {
    segments
        .iter()
        .map(|&segment| assign_speaker(segment, turns))
        .collect()
}

fn assign_speaker(segment: (u32, u32), turns: &[SpeakerTurn]) -> Option<i32> {
    if turns.is_empty() {
        return None;
    }

    let mut overlap_by_speaker: BTreeMap<i32, u32> = BTreeMap::new();
    for turn in turns {
        let overlap = overlap_ms(segment, (turn.start_ms, turn.end_ms));
        *overlap_by_speaker.entry(turn.speaker).or_insert(0) += overlap;
    }

    // BTreeMap iterates in ascending key order, so the first speaker to
    // reach the max overlap is the lowest id on any tie.
    let mut best: Option<(i32, u32)> = None;
    for (&speaker, &overlap) in &overlap_by_speaker {
        let is_new_best = match best {
            None => true,
            Some((_, best_overlap)) => overlap > best_overlap,
        };
        if is_new_best {
            best = Some((speaker, overlap));
        }
    }
    let (best_speaker, best_overlap) =
        best.expect("turns is non-empty, so overlap_by_speaker is too");

    if best_overlap > 0 {
        return Some(best_speaker);
    }

    // No turn overlaps this segment at all — fall back to the single
    // nearest turn (not per-speaker-aggregated), lowest speaker id on a tie.
    let mut nearest: Option<(u32, i32)> = None;
    for turn in turns {
        let gap = gap_ms(segment, (turn.start_ms, turn.end_ms));
        nearest = Some(match nearest {
            None => (gap, turn.speaker),
            Some((best_gap, best_speaker)) => {
                if gap < best_gap || (gap == best_gap && turn.speaker < best_speaker) {
                    (gap, turn.speaker)
                } else {
                    (best_gap, best_speaker)
                }
            }
        });
    }
    nearest.map(|(_, speaker)| speaker)
}

/// Assign `speaker_id` on each of `segments` per `turns`' overlap, in place.
/// A segment whose assignment comes back `None` (e.g. `turns` is empty) is
/// left untouched — this never regresses an already-set `speaker_id`, it
/// only ever fills one in.
pub fn assign_speaker_ids(segments: &mut [transcribe::Segment], turns: &[SpeakerTurn]) {
    let spans: Vec<(u32, u32)> = segments
        .iter()
        .map(|s| {
            (
                s.start_ms.min(u32::MAX as u64) as u32,
                s.end_ms.min(u32::MAX as u64) as u32,
            )
        })
        .collect();
    let assignments = merge_segments_with_turns(&spans, turns);

    for (segment, assignment) in segments.iter_mut().zip(assignments) {
        if let Some(speaker) = assignment {
            segment.speaker_id = Some(speaker);
        }
    }
}

/// Apply the outcome of a spawned diarization task to `segments`: on
/// success, assign speaker ids; on any failure (erroring, panicking, or
/// cancelled) log a warning and leave `segments` speaker-less — diarization
/// failure must never be treated as a transcription failure.
///
/// A fallback warning (the other embedding model was retried after a crash)
/// still assigns speakers — it's informational, not a failure signal.
pub fn apply_diarization_outcome(
    segments: &mut [transcribe::Segment],
    outcome: std::result::Result<
        (Result<Vec<SpeakerTurn>>, Option<String>),
        tokio::task::JoinError,
    >,
) -> Option<String> {
    match outcome {
        Ok((Ok(turns), fallback_warning)) => {
            assign_speaker_ids(segments, &turns);
            fallback_warning
        }
        Ok((Err(e), _fallback_warning)) => {
            log::warn!("diarization unavailable, returning speaker-less segments: {e}");
            Some(format!("Speaker identification is unavailable: {e}"))
        }
        Err(e) => {
            log::warn!("diarization task failed, returning speaker-less segments: {e}");
            Some(format!("Speaker identification failed: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    fn speaker_turn(start_ms: u32, end_ms: u32, speaker: i32) -> SpeakerTurn {
        SpeakerTurn {
            start_ms,
            end_ms,
            speaker,
        }
    }

    // EP/BVA: full overlap, split overlap (tied and non-tied), gap fallback,
    // empty-turns, empty-segments, zero-duration span.

    #[test]
    fn merge_assigns_the_speaker_fully_covering_a_segment() {
        let segments = [(1_000, 2_000)];
        let turns = [speaker_turn(0, 3_000, 1)];

        assert_eq!(merge_segments_with_turns(&segments, &turns), vec![Some(1)]);
    }

    #[test]
    fn merge_breaks_a_tied_overlap_by_lowest_speaker_id() {
        let segments = [(0, 2_000)];
        // Speaker 2 listed first (1000ms overlap), speaker 1 second (1000ms
        // overlap) — equal, so speaker 1 must win regardless of list order.
        let turns = [speaker_turn(0, 1_000, 2), speaker_turn(1_000, 2_000, 1)];

        assert_eq!(merge_segments_with_turns(&segments, &turns), vec![Some(1)]);
    }

    #[test]
    fn merge_assigns_the_speaker_with_strictly_greater_overlap_even_with_a_higher_id() {
        let segments = [(0, 3_000)];
        // Speaker 5 has 2000ms overlap, speaker 3 has 1000ms — 5 must win on
        // overlap even though 3 < 5, proving tie-break isn't overriding a
        // real winner.
        let turns = [speaker_turn(0, 2_000, 5), speaker_turn(2_000, 3_000, 3)];

        assert_eq!(merge_segments_with_turns(&segments, &turns), vec![Some(5)]);
    }

    #[test]
    fn merge_falls_back_to_the_nearest_turn_in_a_gap_between_turns() {
        let turns = [speaker_turn(0, 1_000, 1), speaker_turn(3_000, 4_000, 2)];
        // Gap segment closer to turn 1 (500ms away) than turn 2 (1000ms away).
        let nearer_to_first = [(1_500, 2_000)];
        assert_eq!(
            merge_segments_with_turns(&nearer_to_first, &turns),
            vec![Some(1)]
        );

        // Gap segment after both turns, closer to the second (1000ms away
        // vs 4000ms away from the first).
        let after_both = [(5_000, 5_500)];
        assert_eq!(
            merge_segments_with_turns(&after_both, &turns),
            vec![Some(2)]
        );
    }

    #[test]
    fn merge_breaks_a_tied_gap_distance_by_lowest_speaker_id() {
        // Segment (2_000, 2_100) is exactly 1_000ms from both turns'
        // nearest edges — a genuine tie, not an approximation — and the
        // higher-id speaker is listed first, so this fails if the gap-side
        // tie-break clause is ever dropped or reversed.
        let turns = [speaker_turn(0, 1_000, 9), speaker_turn(3_100, 4_000, 3)];
        let segment = [(2_000, 2_100)];

        assert_eq!(merge_segments_with_turns(&segment, &turns), vec![Some(3)]);
    }

    #[test]
    fn merge_assigns_none_for_every_segment_when_turns_is_empty() {
        let segments = [(0, 1_000), (2_000, 3_000)];

        assert_eq!(merge_segments_with_turns(&segments, &[]), vec![None, None]);
    }

    #[test]
    fn merge_returns_empty_for_empty_segments() {
        let turns = [speaker_turn(0, 1_000, 1)];

        assert_eq!(
            merge_segments_with_turns(&[], &turns),
            Vec::<Option<i32>>::new()
        );
    }

    #[test]
    fn merge_does_not_panic_on_a_zero_duration_segment() {
        let segments = [(1_000, 1_000)];
        let turns = [speaker_turn(0, 2_000, 1)];

        // A degenerate point span overlaps nothing (overlap is always 0 for
        // a zero-width span), so it falls back to the nearest turn — which
        // is turn 1 here (distance 0, since 1000 lies within [0, 2000)).
        assert_eq!(merge_segments_with_turns(&segments, &turns), vec![Some(1)]);
    }

    fn transcript_segment(start_ms: u64, end_ms: u64) -> transcribe::Segment {
        transcribe::Segment {
            start_ms,
            end_ms,
            text: "hello".to_string(),
            speaker_id: None,
        }
    }

    #[test]
    fn assign_speaker_ids_writes_the_merged_assignment_into_each_segment() {
        let mut segments = [
            transcript_segment(0, 1_000),
            transcript_segment(2_000, 3_000),
        ];
        let turns = [speaker_turn(0, 1_500, 7), speaker_turn(1_500, 3_500, 4)];

        assign_speaker_ids(&mut segments, &turns);

        assert_eq!(segments[0].speaker_id, Some(7));
        assert_eq!(segments[1].speaker_id, Some(4));
    }

    #[test]
    fn assign_speaker_ids_leaves_speaker_id_untouched_when_turns_is_empty() {
        let mut segments = [
            transcript_segment(0, 1_000),
            transcript_segment(2_000, 3_000),
        ];

        assign_speaker_ids(&mut segments, &[]);

        assert_eq!(segments[0].speaker_id, None);
        assert_eq!(segments[1].speaker_id, None);
    }

    #[test]
    fn assign_speaker_ids_converts_u64_segment_spans_to_u32_correctly() {
        // Realistic multi-segment, non-zero timestamps well within u32
        // range, to exercise the u64 -> u32 conversion path directly
        // (rather than only ever testing with values that happen to fit
        // trivially, e.g. small round numbers already used above).
        let mut segments = [
            transcript_segment(61_234, 64_987),
            transcript_segment(70_000, 75_500),
        ];
        let turns = [
            speaker_turn(61_000, 65_000, 1),
            speaker_turn(65_000, 76_000, 2),
        ];

        assign_speaker_ids(&mut segments, &turns);

        assert_eq!(segments[0].speaker_id, Some(1));
        assert_eq!(segments[1].speaker_id, Some(2));
    }

    #[tokio::test]
    async fn apply_diarization_outcome_assigns_speakers_and_returns_no_warning_on_success() {
        let mut segments = [transcript_segment(0, 1_000)];
        let turns = vec![speaker_turn(0, 1_000, 3)];

        let warning = apply_diarization_outcome(&mut segments, Ok((Ok(turns), None)));

        assert_eq!(segments[0].speaker_id, Some(3));
        assert_eq!(warning, None);
    }

    #[tokio::test]
    async fn apply_diarization_outcome_assigns_speakers_and_returns_the_fallback_warning() {
        let mut segments = [transcript_segment(0, 1_000)];
        let turns = vec![speaker_turn(0, 1_000, 3)];
        let fallback = "used CAM++ because TitaNet-large failed on this recording".to_string();

        let warning = apply_diarization_outcome(
            &mut segments,
            Ok((Ok(turns.clone()), Some(fallback.clone()))),
        );

        assert_eq!(segments[0].speaker_id, Some(3));
        assert_eq!(warning, Some(fallback));
    }

    #[tokio::test]
    async fn apply_diarization_outcome_leaves_segments_untouched_and_returns_a_warning_on_diarization_asset_error(
    ) {
        let mut segments = [transcript_segment(0, 1_000)];

        let warning = apply_diarization_outcome(
            &mut segments,
            Ok((
                Err(AppError::DiarizationAsset(
                    "models not downloaded".to_string(),
                )),
                None,
            )),
        );

        assert_eq!(segments[0].speaker_id, None);
        assert!(warning.is_some());
    }

    #[tokio::test]
    async fn apply_diarization_outcome_leaves_segments_untouched_and_returns_a_warning_on_engine_error(
    ) {
        let mut segments = [transcript_segment(0, 1_000)];

        let warning = apply_diarization_outcome(
            &mut segments,
            Ok((
                Err(AppError::Diarization("engine exploded".to_string())),
                None,
            )),
        );

        assert_eq!(segments[0].speaker_id, None);
        assert!(warning.is_some());
    }

    #[tokio::test]
    async fn apply_diarization_outcome_leaves_segments_untouched_and_returns_a_warning_when_the_task_panics(
    ) {
        let mut segments = [transcript_segment(0, 1_000)];

        let join_result = tokio::task::spawn_blocking(|| {
            panic!("simulated diarization task panic");
        })
        .await;
        let err = join_result.expect_err("spawned task was made to panic");

        let warning = apply_diarization_outcome(&mut segments, Err(err));

        assert_eq!(segments[0].speaker_id, None);
        assert!(warning.is_some());
    }
}
