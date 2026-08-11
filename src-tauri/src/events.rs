//! Tauri event payloads: emitted on the IPC event bus, consumed by the
//! frontend (`src/ipc.ts`).

use serde::Serialize;

/// Payload of the `transcription_phase` event, emitted once a run moves from
/// transcribing into diarizing its samples. `phase` is a fixed literal today
/// (diarization is the only phase change the UI needs to know about beyond
/// the run simply being in flight) but stays a string, not a bool, so a
/// future phase can be added without changing the event's shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct TranscriptionPhaseEvent {
    pub(crate) id: i64,
    pub(crate) phase: &'static str,
}

/// Whisper's 0–100 completion estimate for the transcription phase of one
/// Meeting run. Diarization has no equivalent estimate.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct TranscriptionProgressEvent {
    pub(crate) id: i64,
    pub(crate) percent: i32,
}

/// Emitted once per decoded Streaming window (`streaming_window`), whether
/// it succeeded or fail-open-skipped — `outcome_ok` distinguishes the two so
/// the UI can show "this span failed" rather than reading a skip as silence.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StreamingWindowEvent {
    pub(crate) session_id: i64,
    pub(crate) window_index: i64,
    pub(crate) start_ms: i64,
    pub(crate) end_ms: i64,
    pub(crate) text: String,
    pub(crate) language: String,
    pub(crate) outcome_ok: bool,
}

/// Emitted once, right after a session starts (`streaming_sources`), naming
/// which capture source(s) actually came up — the mic-only-degradation
/// indicator WP-73's UI needs, since a silent fallback would be invisible.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StreamingSourcesEvent {
    pub(crate) session_id: i64,
    pub(crate) mic: bool,
    pub(crate) system_audio: bool,
}

/// Emitted once the decode loop ends (`streaming_session_ended`) — either
/// because `stop_streaming_session` dropped the capture, or (not yet
/// possible in v1: no auto-timeout) it ended on its own.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StreamingSessionEndedEvent {
    pub(crate) session_id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pins the `transcription_phase` event's wire shape against the
    // frontend's `TranscriptionPhase` interface in `src/ipc.ts` (`{ id:
    // number; phase: "diarizing" }`) so a field rename on either side fails
    // this test instead of silently leaving the frontend's id/phase guard in
    // `App.tsx` unable to match the event.
    #[test]
    fn transcription_phase_event_serializes_with_the_keys_and_casing_the_frontend_expects() {
        let event = TranscriptionPhaseEvent {
            id: 42,
            phase: "diarizing",
        };

        let json = serde_json::to_value(&event).unwrap();

        assert_eq!(json["id"], serde_json::json!(42));
        assert_eq!(json["phase"], serde_json::json!("diarizing"));
        assert_eq!(
            json.as_object().unwrap().len(),
            2,
            "unexpected extra key in the transcription_phase payload"
        );
    }

    #[test]
    fn transcription_progress_event_serializes_with_the_keys_and_casing_the_frontend_expects() {
        let event = TranscriptionProgressEvent {
            id: 42,
            percent: 10,
        };
        let json = serde_json::to_value(&event).unwrap();

        assert_eq!(json["id"], serde_json::json!(42));
        assert_eq!(json["percent"], serde_json::json!(10));
        assert_eq!(json.as_object().unwrap().len(), 2);
    }
}
