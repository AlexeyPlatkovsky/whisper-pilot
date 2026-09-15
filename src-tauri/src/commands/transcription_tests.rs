use super::*;
use std::sync::Arc;

#[test]
fn qwen_file_windows_overlap_without_moving_the_logical_timeline() {
    let sample_rate = crate::audio::SAMPLE_RATE as usize;
    let windows = qwen_window_plan(sample_rate * 61);

    assert_eq!(windows.len(), 3);
    assert_eq!(windows[0], (0, sample_rate * 30, 0, sample_rate * 30));
    assert_eq!(
        windows[1],
        (
            sample_rate * 29,
            sample_rate * 59,
            sample_rate * 30,
            sample_rate * 59
        )
    );
    assert_eq!(
        windows[2],
        (
            sample_rate * 58,
            sample_rate * 61,
            sample_rate * 59,
            sample_rate * 61
        )
    );
}

#[test]
fn qwen_window_segments_keep_relative_timestamps_after_decode_start_offset() {
    let sample_rate = crate::audio::SAMPLE_RATE as usize;
    let mut committed = Vec::new();

    place_qwen_window_segments(
        &mut committed,
        vec![
            segment(500, 1_200, "boundary"),
            segment(1_500, 2_600, "new"),
        ],
        sample_rate * 29,
        sample_rate * 30,
        sample_rate * 59,
    );

    assert_eq!(
        committed
            .iter()
            .map(|segment| (segment.start_ms, segment.end_ms, segment.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(30_000, 30_200, "boundary"), (30_500, 31_600, "new")]
    );
}

#[test]
fn qwen_window_segments_discard_completed_overlap_and_clip_to_logical_end() {
    let sample_rate = crate::audio::SAMPLE_RATE as usize;
    let mut committed = Vec::new();

    place_qwen_window_segments(
        &mut committed,
        vec![segment(0, 900, "old"), segment(29_500, 31_500, "tail")],
        sample_rate * 29,
        sample_rate * 30,
        sample_rate * 59,
    );

    assert_eq!(
        committed
            .iter()
            .map(|segment| (segment.start_ms, segment.end_ms, segment.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(58_500, 59_000, "tail")]
    );
}

#[test]
fn qwen_window_segments_deduplicate_repeated_overlap_output() {
    let sample_rate = crate::audio::SAMPLE_RATE as usize;
    let mut committed = vec![segment(29_500, 30_000, "Repeated phrase")];

    place_qwen_window_segments(
        &mut committed,
        vec![
            segment(500, 1_500, " repeated   PHRASE "),
            segment(1_600, 2_000, "fresh"),
        ],
        sample_rate * 29,
        sample_rate * 30,
        sample_rate * 59,
    );

    assert_eq!(
        committed
            .iter()
            .map(|segment| (segment.start_ms, segment.end_ms, segment.text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (29_500, 30_000, "Repeated phrase"),
            (30_600, 31_000, "fresh")
        ]
    );
}

// EP: "none" is the sole class that skips diarization; every other
// string (however it got there) names a variant to run.
#[test]
fn diarization_variant_to_run_skips_when_setting_is_none() {
    assert_eq!(diarization_variant_to_run("none"), None);
}

#[test]
fn diarization_variant_to_run_passes_through_a_real_variant() {
    assert_eq!(diarization_variant_to_run("campplus"), Some("campplus"));
    assert_eq!(
        diarization_variant_to_run("titanet-large"),
        Some("titanet-large")
    );
}

fn sample_meeting_dto() -> MeetingDto {
    MeetingDto {
        id: 1,
        title: "Test Meeting".to_string(),
        source_path: None,
        source_name: None,
        created_at_ms: 0,
        duration_ms: None,
        language: "en".to_string(),
        status: "transcribed".to_string(),
        segments: Vec::new(),
        mfu: None,
        source_missing: false,
    }
}

#[test]
fn transcribe_meeting_result_round_trips_with_a_diarization_warning() {
    let original = TranscribeMeetingResult {
        meeting: sample_meeting_dto(),
        diarization_warning: Some("active diarization model is missing".to_string()),
    };

    let json = serde_json::to_value(&original).unwrap();
    let round_tripped: TranscribeMeetingResult = serde_json::from_value(json).unwrap();

    assert_eq!(round_tripped, original);
}

#[test]
fn transcribe_meeting_result_omits_diarization_warning_key_when_none() {
    let original = TranscribeMeetingResult {
        meeting: sample_meeting_dto(),
        diarization_warning: None,
    };

    let json = serde_json::to_value(&original).unwrap();

    assert!(json.get("diarization_warning").is_none());
}

#[tokio::test]
async fn meeting_start_claims_busy_before_releasing_asr_selection_barrier() {
    let state = AppState::default();
    let dir = tempfile::tempdir().expect("temporary settings directory");
    let held_mutation = state.recorder_asr_mutation.lock().await;

    let blocked = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        resolve_meeting_asr_and_claim(&state, dir.path()),
    )
    .await;
    assert!(
        blocked.is_err(),
        "selection must wait for a concurrent ASR mutation before it can read settings"
    );

    drop(held_mutation);
    let (_settings, _spec, usage) = resolve_meeting_asr_and_claim(&state, dir.path())
        .await
        .expect("default ASR selection can claim the Meeting slot");
    assert_eq!(
        crate::streaming_session::current_whisper_user(&state.whisper_busy),
        Some(crate::streaming_session::WhisperUser::Meeting),
        "the mutation barrier is released only after the Meeting ownership claim"
    );
    drop(usage);
}

fn segment(start_ms: u64, end_ms: u64, text: &str) -> transcribe::Segment {
    transcribe::Segment {
        start_ms,
        end_ms,
        text: text.to_string(),
        speaker_id: None,
    }
}

fn transcription(segments: Vec<transcribe::Segment>) -> transcribe::Transcription {
    transcribe::Transcription {
        segments,
        language: "en".to_string(),
    }
}

/// A meeting row to persist a transcript against, in a throwaway app-support
/// directory — never the user's own.
fn meeting_in(dir: &std::path::Path) -> i64 {
    crate::meetings::create_empty_meeting(dir, 0).unwrap().id
}

fn diarization(
    outcome: DiarizationOutcome,
) -> Option<std::pin::Pin<Box<dyn std::future::Future<Output = DiarizationOutcome> + Send>>> {
    Some(Box::pin(async move { outcome }))
}

// A zero-segment Meeting decode is not useful output, whether its source
// is silent or the decoder could not recover speech. Fail before an empty,
// finished meeting can be persisted.
#[test]
fn ensure_non_empty_transcript_rejects_a_zero_segment_decode_with_neutral_guidance() {
    let err = ensure_non_empty_transcript(&transcription(Vec::new()))
        .expect_err("an empty decode must fail the Meeting run");
    let message = err.to_string();
    assert!(
        message.contains("audible speech") && !message.contains("Metal encoder fault"),
        "the error should explain empty speech without claiming the fixed Metal fault: {message}"
    );
}

#[test]
fn ensure_non_empty_transcript_passes_a_decode_with_segments() {
    let t = transcription(vec![segment(0, 1_000, "hello")]);
    ensure_non_empty_transcript(&t).expect("a decode with segments passes");
}

// WP-116 DoD 1: moving the decoded allocation into blocking Whisper work
// must not require a full Vec clone merely to retain it for diarization.
// Pointer identity makes that ownership behavior observable without a
// model, allocator instrumentation, or private media.
#[tokio::test]
async fn blocking_transcription_owns_and_returns_the_original_sample_allocation() {
    let samples: Vec<f32> = (0..32_000).map(|sample| sample as f32).collect();
    let original_pointer = samples.as_ptr() as usize;
    let observed_pointer = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&observed_pointer);

    let (sample_count, returned_samples) = transcribe_owned_samples(samples, move |borrowed| {
        observed.store(
            borrowed.as_ptr() as usize,
            std::sync::atomic::Ordering::SeqCst,
        );
        Ok(borrowed.len())
    })
    .await
    .expect("model-free blocking transcription succeeds");

    assert_eq!(sample_count, 32_000);
    assert_eq!(
        observed_pointer.load(std::sync::atomic::Ordering::SeqCst),
        original_pointer
    );
    assert_eq!(returned_samples.as_ptr() as usize, original_pointer);
    assert_eq!(returned_samples.len(), 32_000);
}

// The whole point of WP-54: the transcript must already be readable from
// the store at the moment diarization begins, so a diarization failure of
// any kind — including a native crash that kills the process outright —
// can only ever cost the speaker labels.
#[tokio::test]
async fn persist_transcript_then_diarize_persists_the_transcript_before_diarization_starts() {
    let dir = tempfile::tempdir().unwrap();
    let id = meeting_in(dir.path());
    let observed = Arc::new(std::sync::Mutex::new(None));

    let seen = Arc::clone(&observed);
    let path = dir.path().to_path_buf();
    let (_meeting, warning) = persist_transcript_then_diarize(
        dir.path().to_path_buf(),
        id,
        transcription(vec![
            segment(0, 1_000, "hello"),
            segment(2_000, 3_000, "world"),
        ]),
        Some(Box::pin(async move {
            // Reads the store from inside the diarization pass itself.
            *seen.lock().unwrap() = Some(crate::meetings::open_meeting(&path, id).unwrap());
            Ok((Ok(Vec::new()), None))
        })),
    )
    .await
    .unwrap();

    let at_diarization_time = observed.lock().unwrap().clone().expect("diarization ran");
    assert_eq!(at_diarization_time.segments.len(), 2);
    assert_eq!(at_diarization_time.segments[0].text, "hello");
    assert_eq!(at_diarization_time.language, "en");
    assert_eq!(at_diarization_time.duration_ms, Some(3_000));
    assert!(
        at_diarization_time
            .segments
            .iter()
            .all(|s| s.speaker_id.is_none()),
        "speaker ids are not known until diarization returns"
    );
    assert_eq!(warning, None);
}

#[tokio::test]
async fn persist_transcript_then_diarize_keeps_the_persisted_transcript_when_diarization_fails() {
    let dir = tempfile::tempdir().unwrap();
    let id = meeting_in(dir.path());

    let (_meeting, warning) = persist_transcript_then_diarize(
        dir.path().to_path_buf(),
        id,
        transcription(vec![
            segment(0, 1_000, "hello"),
            segment(2_000, 3_000, "world"),
        ]),
        diarization(Ok((
            Err(AppError::Diarization("engine exploded".to_string())),
            None,
        ))),
    )
    .await
    .unwrap();

    assert!(warning.is_some(), "the failure is reported, not swallowed");
    let reopened = crate::meetings::open_meeting(dir.path(), id).unwrap();
    assert_eq!(reopened.segments.len(), 2);
    assert_eq!(reopened.segments[0].text, "hello");
    assert!(reopened.segments.iter().all(|s| s.speaker_id.is_none()));
}

#[tokio::test]
async fn persist_transcript_then_diarize_writes_speaker_ids_onto_the_persisted_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let id = meeting_in(dir.path());
    // Distinct speakers, so the read-path coalescing in `to_dto` cannot
    // merge the two segments and hide a wrong assignment.
    let turns = vec![
        diarize::SpeakerTurn {
            start_ms: 0,
            end_ms: 1_000,
            speaker: 3,
        },
        diarize::SpeakerTurn {
            start_ms: 2_000,
            end_ms: 3_000,
            speaker: 4,
        },
    ];

    let (meeting, warning) = persist_transcript_then_diarize(
        dir.path().to_path_buf(),
        id,
        transcription(vec![
            segment(0, 1_000, "hello"),
            segment(2_000, 3_000, "world"),
        ]),
        diarization(Ok((Ok(turns), None))),
    )
    .await
    .unwrap();

    assert_eq!(warning, None);
    assert_eq!(meeting.segments[0].speaker_id, Some(3));
    assert_eq!(meeting.segments[1].speaker_id, Some(4));
    let reopened = crate::meetings::open_meeting(dir.path(), id).unwrap();
    assert_eq!(reopened.segments[0].speaker_id, Some(3));
    assert_eq!(reopened.segments[1].speaker_id, Some(4));
    assert_eq!(reopened.language, "en");
    assert_eq!(reopened.duration_ms, Some(3_000));
}

#[tokio::test]
async fn persist_transcript_then_diarize_persists_the_transcript_when_no_model_is_active() {
    let dir = tempfile::tempdir().unwrap();
    let id = meeting_in(dir.path());

    let (meeting, warning) = persist_transcript_then_diarize(
        dir.path().to_path_buf(),
        id,
        transcription(vec![segment(0, 1_000, "hello")]),
        None,
    )
    .await
    .unwrap();

    assert_eq!(warning, None);
    assert_eq!(meeting.segments.len(), 1);
    let reopened = crate::meetings::open_meeting(dir.path(), id).unwrap();
    assert_eq!(reopened.segments.len(), 1);
    assert!(reopened.segments[0].speaker_id.is_none());
}

// Once the transcript is persisted, nothing downstream may turn into a
// failed transcription — including the speaker-id write itself failing.
#[tokio::test]
async fn persist_transcript_then_diarize_warns_instead_of_failing_when_the_speaker_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let id = meeting_in(dir.path());
    let path = dir.path().to_path_buf();

    let (meeting, warning) = persist_transcript_then_diarize(
        dir.path().to_path_buf(),
        id,
        transcription(vec![segment(0, 1_000, "hello")]),
        Some(Box::pin(async move {
            // The meeting disappears after the transcript was persisted but
            // before the speaker ids can be written back to it.
            crate::meetings::delete_meeting(&path, id).unwrap();
            Ok((
                Ok(vec![diarize::SpeakerTurn {
                    start_ms: 0,
                    end_ms: 1_000,
                    speaker: 1,
                }]),
                None,
            ))
        })),
    )
    .await
    .expect("a failed speaker-id write must not fail the transcription");

    assert!(warning.is_some(), "the failed write is reported");
    assert_eq!(
        meeting.segments.len(),
        1,
        "the transcript is still returned"
    );
}

// Error path: the first persist is what fails, so diarization must never
// start — running it would burn minutes of native inference for a result
// that has nowhere to go.
#[tokio::test]
async fn persist_transcript_then_diarize_skips_diarization_when_the_first_persist_fails() {
    let dir = tempfile::tempdir().unwrap();
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let flag = Arc::clone(&ran);
    let error = persist_transcript_then_diarize(
        dir.path().to_path_buf(),
        4_242,
        transcription(vec![segment(0, 1_000, "hello")]),
        Some(Box::pin(async move {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok((Ok(Vec::new()), None))
        })),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, AppError::Store(_)));
    assert!(
        !ran.load(std::sync::atomic::Ordering::SeqCst),
        "diarization must not run once persisting the transcript has failed"
    );
}
