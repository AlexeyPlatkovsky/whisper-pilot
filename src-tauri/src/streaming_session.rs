//! Streaming decode/session pipeline: rolling-window Whisper decode over the
//! continuous sample stream `streaming_audio.rs` produces.
//!
//! Reuses `transcribe::transcribe` unchanged — a window is just a short
//! slice of samples, decoded exactly like a (short) Meeting file, so no new
//! whisper-rs FFI is needed here, only the windowing and session logic
//! around it.
//!
//! **Windowing:** fixed-size, non-overlapping ~`WINDOW_SECONDS` windows.
//! Simpler than a sliding/overlapping window, at the cost of an occasional
//! word split across a window boundary — an accepted, documented trade-off
//! for a first version, not a silent one. Each window gets its own language
//! detection (unlike Meeting's once-per-file detection, ADR-012), because a
//! live session has no single fixed language the way a finished file does.
//!
//! **Fail-open:** a window whose decode errors is skipped (logged, no text
//! emitted for that span) rather than ending the session, mirroring this
//! app's other fail-open engine paths (diarization, ADR-013).
//!
//! **Mutual exclusion:** [`WhisperUsageGuard`] enforces WP-68's decision that
//! a Meeting transcription and a Streaming session cannot run concurrently —
//! both would contend for the one cached Whisper context in `AppState`, and
//! whisper-rs's `WhisperState` is not proven safe for two concurrent
//! `.full()` calls against the same `WhisperContext`.
//!
//! **Not yet wired to Tauri IPC.** This module is the core session
//! machinery; command/event registration ties it to the Streaming UI (WP-73)
//! and lands with that work. **Latency is not measured against real
//! hardware** — the required feasibility spike (see WP-71 in TaskPilot)
//! needs a person running this on real Mac hardware with a downloaded model,
//! which this environment cannot do; `WindowResult` carries `decode_ms` so
//! that measurement is possible once someone can run it.

use crate::audio::SAMPLE_RATE;
use crate::transcribe::{self, Transcription};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use whisper_rs::WhisperContext;

/// Target window length. The midpoint of WP-68's approved 5-10s latency
/// budget — not itself the measured, finalized threshold (that's the
/// feasibility spike's job), just the windowing granularity.
const WINDOW_SECONDS: f64 = 7.0;

const WINDOW_SAMPLES: usize = (WINDOW_SECONDS * SAMPLE_RATE as f64) as usize;

/// A window's length in milliseconds, exposed so a caller building a
/// window's `end_ms` (IPC/persistence, not this module's concern) does not
/// need to duplicate `WINDOW_SECONDS`.
pub const WINDOW_MS: u64 = (WINDOW_SECONDS * 1000.0) as u64;

/// How long the decode loop waits for a sample chunk before checking `stop`
/// again. Independent of `WINDOW_SECONDS` — this is loop responsiveness, not
/// decode granularity.
const RECV_POLL: Duration = Duration::from_millis(200);

const IDLE: u8 = 0;
const MEETING: u8 = 1;
const STREAMING: u8 = 2;

/// Which caller holds (or is asking to hold) the shared Whisper context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperUser {
    Meeting,
    Streaming,
}

impl WhisperUser {
    fn as_u8(self) -> u8 {
        match self {
            Self::Meeting => MEETING,
            Self::Streaming => STREAMING,
        }
    }

    fn from_u8(v: u8) -> Option<Self> {
        match v {
            MEETING => Some(Self::Meeting),
            STREAMING => Some(Self::Streaming),
            _ => None,
        }
    }
}

/// RAII hold on the shared Whisper context, released on drop. Construct via
/// [`WhisperUsageGuard::acquire`]; there is no other way to make one, so a
/// held guard is always a real, exclusive hold.
#[derive(Debug)]
pub struct WhisperUsageGuard<'a> {
    state: &'a AtomicU8,
}

impl<'a> WhisperUsageGuard<'a> {
    /// Attempts to acquire `state` for `user`. On contention, names who
    /// currently holds it (falling back to the requester if the holder's
    /// encoding is somehow unrecognized, which cannot happen through this
    /// API but must still return something rather than panic).
    pub fn acquire(state: &'a AtomicU8, user: WhisperUser) -> Result<Self, WhisperUser> {
        match state.compare_exchange(IDLE, user.as_u8(), Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => Ok(Self { state }),
            Err(current) => Err(WhisperUser::from_u8(current).unwrap_or(user)),
        }
    }
}

impl Drop for WhisperUsageGuard<'_> {
    fn drop(&mut self) {
        self.state.store(IDLE, Ordering::Release);
    }
}

/// Claim `state` for a Streaming session that outlives one IPC command call
/// (`start_streaming_session` returns immediately; the claim must survive
/// until `stop_streaming_session`, a separate call, later runs) — the
/// borrowed-lifetime [`WhisperUsageGuard`] cannot express that, so this pair
/// of free functions performs the same compare-exchange/release directly.
/// Prefer `WhisperUsageGuard` whenever a hold is scoped to one function.
pub fn try_claim_streaming(state: &AtomicU8) -> Result<(), WhisperUser> {
    match state.compare_exchange(
        IDLE,
        WhisperUser::Streaming.as_u8(),
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => Ok(()),
        Err(current) => Err(WhisperUser::from_u8(current).unwrap_or(WhisperUser::Streaming)),
    }
}

/// Releases whichever user currently holds `state`. Pairs with
/// [`try_claim_streaming`] (and, in principle, a leaked `WhisperUsageGuard`,
/// though nothing in this codebase does that).
pub fn release_whisper_busy(state: &AtomicU8) {
    state.store(IDLE, Ordering::Release);
}

/// One decoded window, or the error it failed with (fail-open — the session
/// keeps running either way). `decode_ms` supports the still-outstanding
/// feasibility spike measuring real per-window latency.
pub struct WindowResult {
    pub window_index: u64,
    pub start_ms: u64,
    pub decode_ms: u64,
    pub outcome: crate::error::Result<Transcription>,
}

/// Take exactly one window's worth of samples off the front of `buffer` when
/// enough have accumulated, leaving any remainder for the next call. Pure —
/// no I/O, no time — so windowing math is unit-testable without a model.
fn take_window(buffer: &mut Vec<f32>) -> Option<Vec<f32>> {
    if buffer.len() < WINDOW_SAMPLES {
        return None;
    }
    Some(buffer.drain(..WINDOW_SAMPLES).collect())
}

/// Milliseconds into the session that window `window_index` starts, given
/// fixed-size non-overlapping windows.
fn window_start_ms(window_index: u64) -> u64 {
    window_index * (WINDOW_SECONDS * 1000.0) as u64
}

/// Runs the rolling-window decode loop until `samples_rx` disconnects (the
/// capture session ended) or a result fails to send (the consumer is gone).
/// Blocking — call from `tokio::task::spawn_blocking`, matching every other
/// heavy Rust-core operation in this app (model load, Meeting transcription,
/// diarization).
///
/// Takes `ctx` already-acquired: this function does not itself enforce
/// mutual exclusion — the caller must hold a live [`WhisperUsageGuard`] for
/// [`WhisperUser::Streaming`] for the loop's whole duration, exactly as
/// `transcribe_meeting` will need to hold one for [`WhisperUser::Meeting`].
pub fn run_windowed_decode(
    ctx: Arc<WhisperContext>,
    samples_rx: Receiver<Vec<f32>>,
    results_tx: Sender<WindowResult>,
) {
    let mut buffer: Vec<f32> = Vec::new();
    let mut window_index: u64 = 0;

    loop {
        match samples_rx.recv_timeout(RECV_POLL) {
            Ok(chunk) => buffer.extend(chunk),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }

        while let Some(window) = take_window(&mut buffer) {
            let start_ms = window_start_ms(window_index);
            let decode_start = Instant::now();
            // Fail-open (module doc): an Err here is forwarded, not
            // propagated — the caller skips this window's text and the loop
            // keeps running on the next one.
            let outcome = transcribe::transcribe(&ctx, &window);
            let decode_ms = decode_start.elapsed().as_millis() as u64;

            if results_tx
                .send(WindowResult {
                    window_index,
                    start_ms,
                    decode_ms,
                    outcome,
                })
                .is_err()
            {
                return;
            }
            window_index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_window_returns_none_below_the_threshold() {
        let mut buffer = vec![0.0_f32; WINDOW_SAMPLES - 1];
        assert!(take_window(&mut buffer).is_none());
        // Nothing was drained.
        assert_eq!(buffer.len(), WINDOW_SAMPLES - 1);
    }

    #[test]
    fn take_window_drains_exactly_one_window_and_keeps_the_remainder() {
        let mut buffer = vec![0.0_f32; WINDOW_SAMPLES + 100];
        let window = take_window(&mut buffer).expect("buffer has a full window");
        assert_eq!(window.len(), WINDOW_SAMPLES);
        assert_eq!(buffer.len(), 100);
    }

    #[test]
    fn take_window_preserves_sample_order() {
        let mut buffer: Vec<f32> = (0..WINDOW_SAMPLES as u32 + 5).map(|i| i as f32).collect();
        let window = take_window(&mut buffer).unwrap();
        assert_eq!(window[0], 0.0);
        assert_eq!(window[WINDOW_SAMPLES - 1], (WINDOW_SAMPLES - 1) as f32);
        // Remainder starts where the window left off.
        assert_eq!(buffer[0], WINDOW_SAMPLES as f32);
    }

    #[test]
    fn take_window_can_be_called_repeatedly_on_a_multi_window_buffer() {
        let mut buffer = vec![0.0_f32; WINDOW_SAMPLES * 2 + 1];
        assert!(take_window(&mut buffer).is_some());
        assert!(take_window(&mut buffer).is_some());
        assert!(take_window(&mut buffer).is_none());
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn window_start_ms_is_zero_for_the_first_window() {
        assert_eq!(window_start_ms(0), 0);
    }

    #[test]
    fn window_start_ms_advances_by_the_window_length_each_time() {
        let first = window_start_ms(0);
        let second = window_start_ms(1);
        assert_eq!(second - first, (WINDOW_SECONDS * 1000.0) as u64);
    }

    #[test]
    fn whisper_usage_guard_grants_exclusive_access() {
        let state = AtomicU8::new(IDLE);

        let guard = WhisperUsageGuard::acquire(&state, WhisperUser::Streaming)
            .expect("idle state must grant the first acquire");

        let contended = WhisperUsageGuard::acquire(&state, WhisperUser::Meeting);
        assert_eq!(
            contended.expect_err("a held guard must block a second acquire"),
            WhisperUser::Streaming,
            "the error must name who is currently holding it"
        );

        drop(guard);

        WhisperUsageGuard::acquire(&state, WhisperUser::Meeting)
            .expect("releasing the first guard must allow a new acquire");
    }

    #[test]
    fn whisper_usage_guard_releases_on_drop_even_after_a_contended_attempt() {
        let state = AtomicU8::new(IDLE);
        {
            let _guard = WhisperUsageGuard::acquire(&state, WhisperUser::Streaming).unwrap();
            assert!(WhisperUsageGuard::acquire(&state, WhisperUser::Streaming).is_err());
        }
        assert!(WhisperUsageGuard::acquire(&state, WhisperUser::Streaming).is_ok());
    }

    #[test]
    fn try_claim_streaming_then_release_allows_a_later_claim() {
        let state = AtomicU8::new(IDLE);

        try_claim_streaming(&state).expect("idle state must grant the claim");
        assert_eq!(
            try_claim_streaming(&state).expect_err("a held claim must block a second one"),
            WhisperUser::Streaming
        );

        release_whisper_busy(&state);

        try_claim_streaming(&state).expect("releasing must allow a new claim");
    }

    #[test]
    fn try_claim_streaming_names_a_meeting_holder() {
        let state = AtomicU8::new(IDLE);
        let _guard = WhisperUsageGuard::acquire(&state, WhisperUser::Meeting).unwrap();

        assert_eq!(
            try_claim_streaming(&state).expect_err("a Meeting hold must block Streaming"),
            WhisperUser::Meeting
        );
    }

    #[test]
    fn window_ms_matches_window_seconds_in_milliseconds() {
        assert_eq!(WINDOW_MS, (WINDOW_SECONDS * 1000.0) as u64);
    }
}
