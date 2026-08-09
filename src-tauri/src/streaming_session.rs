//! Rolling-window Streaming decode. See `docs/architecture.md`'s Streaming
//! Decode/Session Pipeline section for lifecycle and trade-off details.

use crate::audio::SAMPLE_RATE;
use crate::error::AppError;
use crate::transcribe::{self, Transcription};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use whisper_rs::{WhisperContext, WhisperState};

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

/// Decodes one window at a time for a streaming session's whole lifetime.
/// The seam that lets the decode loop own "one whisper state per session"
/// (WP-82) while tests substitute a model-free double.
pub trait SessionDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        cancel: &Arc<AtomicBool>,
    ) -> crate::error::Result<Transcription>;
}

/// The production [`SessionDecoder`]: owns the session's single
/// `WhisperState`, created once via [`WhisperSessionDecoder::new`] and reused
/// for every window (`transcribe::transcribe_with_state`; state reuse across
/// calls is upstream's own `whisper_full` pattern).
pub struct WhisperSessionDecoder {
    state: WhisperState,
}

impl WhisperSessionDecoder {
    pub fn new(ctx: &WhisperContext) -> crate::error::Result<Self> {
        let state = ctx
            .create_state()
            .map_err(|e| AppError::Transcribe(e.to_string()))?;
        Ok(Self { state })
    }
}

impl SessionDecoder for WhisperSessionDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        cancel: &Arc<AtomicBool>,
    ) -> crate::error::Result<Transcription> {
        transcribe::transcribe_with_state(&mut self.state, samples, cancel, |_| {})
    }
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

/// Blocking decode loop; call from `spawn_blocking` while holding the Streaming
/// claim. Its decoder is created once and creation failure fails open per window.
/// `starting_window_index` preserves timeline continuity after a resume.
pub fn run_windowed_decode<D, F>(
    make_decoder: F,
    samples_rx: Receiver<Vec<f32>>,
    results_tx: Sender<WindowResult>,
    starting_window_index: u64,
) where
    D: SessionDecoder,
    F: FnOnce() -> crate::error::Result<D>,
{
    let mut buffer: Vec<f32> = Vec::new();
    let mut window_index: u64 = starting_window_index;
    // Streaming windows have no per-window Stop control (WP-19 targets
    // Meeting transcription only); a session ends by dropping capture, so
    // each window's decode always runs to completion.
    let never_cancel = Arc::new(AtomicBool::new(false));
    // Kept as the message rather than the AppError (not Clone): a creation
    // failure is rebuilt per window so each fail-open result reads exactly
    // like a per-window decode failure.
    let mut decoder = make_decoder().map_err(|e| e.to_string());

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
            let outcome = match &mut decoder {
                Ok(decoder) => decoder.decode_window(&window, &never_cancel),
                Err(message) => Err(AppError::Transcribe(message.clone())),
            };
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

    // WP-82: the decode loop must create ONE decoder (one WhisperState, one
    // Metal backend) per session and reuse it for every window — not one per
    // window, which put a full Metal init/free cycle on every 7s window.
    struct FakeDecoder {
        decoded: Vec<usize>,
    }

    impl SessionDecoder for FakeDecoder {
        fn decode_window(
            &mut self,
            samples: &[f32],
            _cancel: &Arc<AtomicBool>,
        ) -> crate::error::Result<Transcription> {
            self.decoded.push(samples.len());
            Ok(Transcription {
                segments: vec![],
                language: "en".to_string(),
            })
        }
    }

    fn two_windows_channel() -> (
        std::sync::mpsc::Sender<Vec<f32>>,
        std::sync::mpsc::Receiver<Vec<f32>>,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(vec![0.0_f32; WINDOW_SAMPLES * 2])
            .expect("send two windows of samples");
        (tx, rx)
    }

    #[test]
    fn decode_loop_creates_one_decoder_for_a_multi_window_session() {
        let creations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (samples_tx, samples_rx) = two_windows_channel();
        let (results_tx, results_rx) = std::sync::mpsc::channel();
        drop(samples_tx);

        let counter = Arc::clone(&creations);
        run_windowed_decode(
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(FakeDecoder {
                    decoded: Vec::new(),
                })
            },
            samples_rx,
            results_tx,
            0,
        );

        assert_eq!(
            creations.load(Ordering::SeqCst),
            1,
            "a session creates its decoder exactly once, however many windows it decodes"
        );
        let results: Vec<_> = results_rx.try_iter().collect();
        assert_eq!(results.len(), 2);
        assert!(
            results.iter().all(|r| r.outcome.is_ok()),
            "every window decoded through the one session decoder"
        );
        assert_eq!(results[0].window_index, 0);
        assert_eq!(results[1].window_index, 1);
    }

    #[test]
    fn decode_loop_fail_opens_every_window_when_decoder_creation_fails() {
        let (samples_tx, samples_rx) = two_windows_channel();
        let (results_tx, results_rx) = std::sync::mpsc::channel();
        drop(samples_tx);

        run_windowed_decode(
            || -> crate::error::Result<FakeDecoder> {
                Err(crate::error::AppError::Transcribe(
                    "Metal backend init failed".to_string(),
                ))
            },
            samples_rx,
            results_tx,
            0,
        );

        let results: Vec<_> = results_rx.try_iter().collect();
        assert_eq!(
            results.len(),
            2,
            "a session whose decoder cannot be created still answers every window"
        );
        for result in &results {
            assert!(
                matches!(result.outcome, Err(crate::error::AppError::Transcribe(_))),
                "creation failure must fail each window open, not end the session"
            );
        }
    }

    // A session stopped before one full window accumulates: the decoder is
    // still created once up front, the partial window is dropped (unchanged
    // pre-WP-82 behavior), and the loop returns cleanly on disconnect.
    #[test]
    fn decode_loop_creates_the_decoder_once_even_when_no_window_completes() {
        let creations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        samples_tx
            .send(vec![0.0_f32; WINDOW_SAMPLES - 1])
            .expect("send a partial window");
        drop(samples_tx);
        let (results_tx, results_rx) = std::sync::mpsc::channel();

        let counter = Arc::clone(&creations);
        run_windowed_decode(
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(FakeDecoder {
                    decoded: Vec::new(),
                })
            },
            samples_rx,
            results_tx,
            0,
        );

        assert_eq!(creations.load(Ordering::SeqCst), 1);
        assert!(
            results_rx.try_iter().next().is_none(),
            "a partial trailing window is not decoded"
        );
    }
}
