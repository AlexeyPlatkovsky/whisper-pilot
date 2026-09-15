use super::{decoder_exit_failure, recorder_result_error_is_terminal, try_forward_recorder_asr};
use crate::commands::recorder_dto::RecorderSessionSummaryDto;
use crate::recorder_store::RecorderStatus;
use crate::streaming_audio::CapturedAudioChunk;
use crate::streaming_session::WindowResultKind;
use std::sync::mpsc::sync_channel;

#[test]
fn recorder_summary_serializes_draft_state_for_the_renderer() {
    let summary = RecorderSessionSummaryDto {
        id: 7,
        title: "Draft".into(),
        created_at_ms: 10,
        updated_at_ms: 10,
        duration_ms: 0,
        status: RecorderStatus::Completed,
        is_draft: true,
        sample_rate: 48_000,
        recovery_reason: None,
        audio_path: "/tmp/7.caf".into(),
        asr_model_id: "transcription".into(),
        asr_engine: "whisper".into(),
        asr_language: "auto".into(),
    };
    let json = serde_json::to_value(summary).unwrap();
    assert_eq!(json["is_draft"], true);
    assert_eq!(json["status"], "completed");
}

#[test]
fn only_committed_decode_errors_make_recorder_recoverable() {
    assert!(recorder_result_error_is_terminal(
        WindowResultKind::Committed
    ));
    assert!(!recorder_result_error_is_terminal(WindowResultKind::Gap));
    assert!(!recorder_result_error_is_terminal(
        WindowResultKind::Partial
    ));
}

#[test]
fn full_realtime_asr_queue_never_blocks_or_fails_audio_persistence() {
    let (tx, _rx) = sync_channel(1);
    tx.send(CapturedAudioChunk {
        start_sample: 0,
        captured_end_sample: 1,
        samples: vec![0.1],
    })
    .unwrap();
    let forwarded = try_forward_recorder_asr(
        &tx,
        CapturedAudioChunk {
            start_sample: 1,
            captured_end_sample: 2,
            samples: vec![0.2],
        },
    )
    .expect("a full live-ASR queue is a recoverable preview drop");
    assert!(!forwarded, "the overloaded realtime chunk is dropped");
}

#[test]
fn decoder_panic_becomes_a_synchronized_recoverable_failure() {
    let outcome = std::panic::catch_unwind(|| panic!("decoder failure"));
    let message = decoder_exit_failure(&outcome).expect("panic must be terminal");
    assert!(message.contains("audio was preserved for recovery"));
}
