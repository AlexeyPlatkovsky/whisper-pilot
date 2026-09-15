//! Rolling-window Streaming decode. See `docs/architecture.md`'s Streaming
//! Decode/Session Pipeline section for lifecycle and trade-off details.

use crate::audio::SAMPLE_RATE;
use crate::error::AppError;
use crate::streaming_audio::CapturedAudioChunk;
use crate::transcribe::{self, Transcription};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use whisper_rs::{WhisperContext, WhisperState};

#[path = "streaming_session/decoder.rs"]
mod decoder;
#[path = "streaming_session/results.rs"]
mod results;
#[cfg(test)]
use decoder::whisper_streaming_prompt;
#[cfg(target_os = "macos")]
pub use decoder::QwenGgufSessionDecoder;
pub use decoder::WhisperSessionDecoder;
pub use results::{
    result_channel, ResultQueueTerminalError, ResultReceiver, ResultSender,
    COMMITTED_RESULT_QUEUE_CAPACITY,
};

/// Hard safety cap for uninterrupted speech. Normal utterances commit sooner
/// at a natural trailing pause; the cap prevents unbounded model inputs when
/// a speaker never pauses.
const WINDOW_SECONDS: f64 = 20.0;

const WINDOW_SAMPLES: usize = (WINDOW_SECONDS * SAMPLE_RATE as f64) as usize;

/// Decode a provisional utterance early enough to feel live without keeping
/// the local model permanently backlogged behind one-second revisions.
const PARTIAL_MIN_SAMPLES: usize = SAMPLE_RATE as usize * 2;
const PARTIAL_STEP_SAMPLES: usize = SAMPLE_RATE as usize * 2;
const NATURAL_COMMIT_MIN_SAMPLES: usize = SAMPLE_RATE as usize * 2;
const NATURAL_PAUSE_SAMPLES: usize = SAMPLE_RATE as usize * 3 / 5;
const VAD_SEARCH_SAMPLES: usize = SAMPLE_RATE as usize * 2;
const VAD_FRAME_SAMPLES: usize = SAMPLE_RATE as usize / 50;
const VAD_MIN_SILENCE_SAMPLES: usize = SAMPLE_RATE as usize / 10;
const VAD_SILENCE_RMS: f64 = 0.003;
const VAD_MAX_SILENCE_RMS: f64 = 0.012;
const VAD_SPEECH_REFERENCE_SAMPLES: usize = SAMPLE_RATE as usize;
const VAD_SPEECH_TO_SILENCE_RATIO: f64 = 0.25;
const HARD_BOUNDARY_OVERLAP_SAMPLES: usize = SAMPLE_RATE as usize * 3 / 4;
const CONTEXT_MAX_CHARS: usize = 240;
const WHISPER_PUNCTUATION_SEED: &str =
    "Здравствуйте! Это пример текста с правильной пунктуацией. Hello! This text uses correct punctuation.";

/// A half-second is long enough to preserve a spoken trailing word without
/// decoding callback noise at shutdown.
const FINAL_MIN_SAMPLES: usize = SAMPLE_RATE as usize / 2;
const MEANINGFUL_AUDIO_RMS: f64 = 0.001;
const VAD_STEADY_NOISE_MAX_RMS: f64 = 0.012;
const VAD_STEADY_NOISE_MAX_RELATIVE_RANGE: f64 = 0.08;

/// Provisional output is advisory: persistence and the renderer only need
/// the newest revision. Never let a slow UI make a later quality decode wait
/// behind an already-obsolete partial result.
fn try_send_partial(tx: &ResultSender, result: WindowResult) -> bool {
    tx.send_partial(result).is_ok()
}

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
const RECORDER: u8 = 3;

/// Which caller holds (or is asking to hold) the shared Whisper context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperUser {
    Meeting,
    Streaming,
    Recorder,
}

impl WhisperUser {
    fn as_u8(self) -> u8 {
        match self {
            Self::Meeting => MEETING,
            Self::Streaming => STREAMING,
            Self::Recorder => RECORDER,
        }
    }

    fn from_u8(v: u8) -> Option<Self> {
        match v {
            MEETING => Some(Self::Meeting),
            STREAMING => Some(Self::Streaming),
            RECORDER => Some(Self::Recorder),
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

pub fn try_claim_recorder(state: &AtomicU8) -> Result<(), WhisperUser> {
    match state.compare_exchange(
        IDLE,
        WhisperUser::Recorder.as_u8(),
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => Ok(()),
        Err(current) => Err(WhisperUser::from_u8(current).unwrap_or(WhisperUser::Recorder)),
    }
}

/// Read the current exclusive transcription owner without changing it. Model
/// mutation uses this to distinguish a visible Error snapshot from a Recorder
/// pipeline that is still draining and owns its ASR assets.
pub fn current_whisper_user(state: &AtomicU8) -> Option<WhisperUser> {
    WhisperUser::from_u8(state.load(Ordering::Acquire))
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowResultKind {
    Partial,
    Committed,
    Gap,
}

pub type SessionDecodeProfile = transcribe::DecodeProfile;

pub struct WindowResult {
    pub kind: WindowResultKind,
    pub window_index: u64,
    pub start_ms: u64,
    pub end_ms: u64,
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
        context: Option<&str>,
        profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription>;

    /// Whether segment timestamps are precise enough to prove that a segment
    /// belongs wholly to prepended boundary-overlap audio.
    fn has_reliable_segment_timestamps(&self) -> bool {
        false
    }
}

/// Take exactly one window's worth of samples off the front of `buffer` when
/// enough have accumulated, leaving any remainder for the next call. Pure —
/// no I/O, no time — so windowing math is unit-testable without a model.
#[cfg(test)]
fn take_window(buffer: &mut Vec<f32>) -> Option<Vec<f32>> {
    if buffer.len() < WINDOW_SAMPLES {
        return None;
    }
    Some(buffer.drain(..WINDOW_SAMPLES).collect())
}

/// Commit a spoken utterance as soon as it is followed by a 600 ms pause.
/// For uninterrupted speech, prefer a short quiet boundary near the hard
/// cap, otherwise split exactly at the cap.
#[cfg(test)]
fn take_commit_window(buffer: &mut Vec<f32>) -> Option<Vec<f32>> {
    take_commit_window_from(buffer, NATURAL_COMMIT_MIN_SAMPLES)
}

/// Same boundary selection as [`take_commit_window`], but scans only the
/// newly appended portion of a still-open utterance (plus one pause-sized
/// lookback). The decode loop advances this cursor after every chunk, so a
/// speaker who talks for twenty seconds does not repeatedly rescan the first
/// nineteen seconds of the growing buffer.
fn take_commit_window_from(buffer: &mut Vec<f32>, natural_scan_start: usize) -> Option<Vec<f32>> {
    let scan_end = buffer.len().min(WINDOW_SAMPLES);
    if scan_end >= NATURAL_COMMIT_MIN_SAMPLES + NATURAL_PAUSE_SAMPLES {
        let mut quiet_start: Option<usize> = None;
        let mut quiet_threshold = VAD_SILENCE_RMS;
        let scan_start = natural_scan_start
            .saturating_sub(NATURAL_PAUSE_SAMPLES + VAD_FRAME_SAMPLES)
            .max(NATURAL_COMMIT_MIN_SAMPLES);
        let aligned_start = scan_start - (scan_start % VAD_FRAME_SAMPLES);
        for start in (aligned_start..scan_end).step_by(VAD_FRAME_SAMPLES) {
            let end = (start + VAD_FRAME_SAMPLES).min(scan_end);
            let frame_rms = root_mean_square(&buffer[start..end]);
            if quiet_start.is_none() {
                quiet_threshold = silence_threshold_before(buffer, start);
            }
            if frame_rms <= quiet_threshold {
                let run_start = *quiet_start.get_or_insert(start);
                if end.saturating_sub(run_start) >= NATURAL_PAUSE_SAMPLES
                    && contains_likely_speech(&buffer[..run_start])
                {
                    return Some(buffer.drain(..end).collect());
                }
            } else {
                quiet_start = None;
            }
        }
    }

    if buffer.len() < WINDOW_SAMPLES {
        return None;
    }
    let search_start = WINDOW_SAMPLES.saturating_sub(VAD_SEARCH_SAMPLES);
    let mut quietest: Option<(usize, f64, f64)> = None;
    for start in
        (search_start..=WINDOW_SAMPLES - VAD_MIN_SILENCE_SAMPLES).step_by(VAD_FRAME_SAMPLES)
    {
        let end = start + VAD_MIN_SILENCE_SAMPLES;
        let rms = root_mean_square(&buffer[start..end]);
        if quietest.map_or(true, |(_, current, _)| rms < current) {
            quietest = Some((end, rms, silence_threshold_before(buffer, start)));
        }
    }
    let boundary = quietest
        .filter(|(_, rms, threshold)| *rms <= *threshold)
        .map_or(WINDOW_SAMPLES, |(end, _, _)| end);
    Some(buffer.drain(..boundary).collect())
}

/// Derive a pause threshold from the second immediately before a candidate
/// boundary. This remains stable after a long utterance, unlike a percentile
/// over the whole growing window, while steady room noise cannot bootstrap
/// itself into being classified as speech followed by silence.
fn silence_threshold_before(samples: &[f32], boundary: usize) -> f64 {
    let reference_start = boundary.saturating_sub(VAD_SPEECH_REFERENCE_SAMPLES);
    let reference_rms = root_mean_square(&samples[reference_start..boundary]);
    (reference_rms * VAD_SPEECH_TO_SILENCE_RATIO).clamp(VAD_SILENCE_RMS, VAD_MAX_SILENCE_RMS)
}

/// Milliseconds into the session that window `window_index` starts, given
/// fixed-size non-overlapping windows.
fn window_start_ms(window_index: u64) -> u64 {
    window_index * (WINDOW_SECONDS * 1000.0) as u64
}

fn samples_to_ms(samples: usize) -> u64 {
    samples as u64 * 1_000 / SAMPLE_RATE as u64
}

/// Conservative energy-only gate for deciding whether ASR should run at all.
/// It rejects only silence/below-threshold input and confidently stationary
/// low-level room or fan noise. Any varying low-gain signal fails safe toward
/// decode because silently deleting quiet speech is worse than a possible
/// noise-window hallucination.
fn contains_likely_speech(samples: &[f32]) -> bool {
    let mut minimum = f64::MAX;
    let mut maximum = 0.0_f64;
    for frame in samples.chunks(VAD_FRAME_SAMPLES) {
        if frame.is_empty() {
            continue;
        }
        let rms = root_mean_square(frame);
        minimum = minimum.min(rms);
        maximum = maximum.max(rms);
    }
    if maximum < MEANINGFUL_AUDIO_RMS {
        return false;
    }
    let relative_range = (maximum - minimum) / maximum;
    maximum > VAD_STEADY_NOISE_MAX_RMS || relative_range > VAD_STEADY_NOISE_MAX_RELATIVE_RANGE
}

fn root_mean_square(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mean_square = samples
        .iter()
        .map(|sample| {
            let sample = *sample as f64;
            sample * sample
        })
        .sum::<f64>()
        / samples.len() as f64;
    mean_square.sqrt()
}

// These explicit decode coordinates make timestamp and overlap ownership
// visible at every hot-loop call site.
#[allow(clippy::too_many_arguments)]
fn decode_result<D: SessionDecoder>(
    decoder: &mut std::result::Result<D, String>,
    samples: &[f32],
    logical_sample_count: usize,
    kind: WindowResultKind,
    window_index: u64,
    start_ms: u64,
    context: Option<&str>,
    leading_overlap_samples: usize,
) -> WindowResult {
    let decode_start = Instant::now();
    let profile = match kind {
        WindowResultKind::Partial => SessionDecodeProfile::FastPartial,
        WindowResultKind::Committed | WindowResultKind::Gap => SessionDecodeProfile::Quality,
    };
    let timestamps_reliable = decoder
        .as_ref()
        .is_ok_and(SessionDecoder::has_reliable_segment_timestamps);
    let mut outcome = match decoder {
        Ok(decoder) => decoder.decode_window(samples, context, profile),
        Err(message) => Err(AppError::Transcribe(message.clone())),
    };
    if leading_overlap_samples > 0 && timestamps_reliable {
        if let Ok(transcription) = outcome.as_mut() {
            transcribe::remove_timestamped_audio_overlap(
                transcription,
                samples_to_ms(leading_overlap_samples),
            );
        }
    }
    WindowResult {
        kind,
        window_index,
        start_ms,
        end_ms: start_ms + samples_to_ms(logical_sample_count),
        decode_ms: decode_start.elapsed().as_millis() as u64,
        outcome,
    }
}

/// Blocking decode loop; call from `spawn_blocking` while holding the Streaming
/// claim. Its decoder is created once and creation failure fails open per window.
/// `starting_window_index` preserves timeline continuity after a resume.
pub fn run_windowed_decode<D, F>(
    make_decoder: F,
    samples_rx: Receiver<CapturedAudioChunk>,
    results_tx: ResultSender,
    starting_window_index: u64,
) where
    D: SessionDecoder,
    F: FnOnce() -> crate::error::Result<D>,
{
    run_windowed_decode_from(
        make_decoder,
        samples_rx,
        results_tx,
        starting_window_index,
        window_start_ms(starting_window_index),
    );
}

/// Variant used by resumed sessions whose last persisted end timestamp may
/// not align to the fixed window size (for example, a committed stop-tail).
pub fn run_windowed_decode_from<D, F>(
    make_decoder: F,
    samples_rx: Receiver<CapturedAudioChunk>,
    results_tx: ResultSender,
    starting_window_index: u64,
    timeline_start_ms: u64,
) where
    D: SessionDecoder,
    F: FnOnce() -> crate::error::Result<D>,
{
    let mut buffer: Vec<f32> = Vec::new();
    let mut window_index: u64 = starting_window_index;
    let mut buffer_start_sample = 0_u64;
    let mut expected_input_sample = 0_u64;
    let mut last_partial_len = 0_usize;
    // Cursor for incremental pause detection. It is reset whenever the front
    // of the buffer moves, and otherwise only the newly appended audio plus a
    // small pause lookback is scanned.
    let mut vad_scanned_len = 0_usize;
    let mut committed_context = String::new();
    let mut boundary_overlap = Vec::<f32>::new();
    // Kept as the message rather than the AppError (not Clone): a creation
    // failure is rebuilt per window so each fail-open result reads exactly
    // like a per-window decode failure.
    let mut decoder = make_decoder().map_err(|e| e.to_string());

    loop {
        let chunk = match samples_rx.recv_timeout(RECV_POLL) {
            Ok(chunk) => chunk,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                let _ = flush_trailing_buffer(
                    &mut decoder,
                    &mut buffer,
                    &results_tx,
                    &mut window_index,
                    timeline_start_ms,
                    buffer_start_sample,
                    &mut committed_context,
                    &mut boundary_overlap,
                );
                break;
            }
        };

        if chunk.start_sample > expected_input_sample {
            let gap_start_sample = if !buffer.is_empty()
                && buffer.len() < FINAL_MIN_SAMPLES
                && contains_likely_speech(&buffer)
            {
                buffer_start_sample
            } else {
                expected_input_sample
            };
            if !flush_trailing_buffer(
                &mut decoder,
                &mut buffer,
                &results_tx,
                &mut window_index,
                timeline_start_ms,
                buffer_start_sample,
                &mut committed_context,
                &mut boundary_overlap,
            ) || !send_capture_gap(
                &results_tx,
                &mut window_index,
                timeline_start_ms,
                gap_start_sample,
                chunk.start_sample,
            ) {
                return;
            }
            buffer_start_sample = chunk.start_sample;
            last_partial_len = 0;
            vad_scanned_len = 0;
            boundary_overlap.clear();
        }

        let overlap = expected_input_sample.saturating_sub(chunk.start_sample) as usize;
        let samples = chunk
            .samples
            .get(overlap.min(chunk.samples.len())..)
            .unwrap_or(&[]);
        if buffer.is_empty() && !samples.is_empty() {
            buffer_start_sample = chunk.start_sample.saturating_add(overlap as u64);
        }
        buffer.extend_from_slice(samples);
        let delivered_end = chunk
            .start_sample
            .saturating_add(chunk.samples.len() as u64);
        expected_input_sample = expected_input_sample.max(delivered_end);

        // Keep the rolling buffer bounded without invoking ASR on a full
        // hard-cap span that never established speech. Advancing the sample
        // clock preserves the position of any later real utterance.
        while buffer.len() >= WINDOW_SAMPLES && !contains_likely_speech(&buffer[..WINDOW_SAMPLES]) {
            buffer.drain(..WINDOW_SAMPLES);
            buffer_start_sample = buffer_start_sample.saturating_add(WINDOW_SAMPLES as u64);
            boundary_overlap.clear();
            last_partial_len = 0;
            vad_scanned_len = 0;
        }

        while let Some(window) = take_commit_window_from(&mut buffer, vad_scanned_len) {
            let start_ms =
                timeline_start_ms.saturating_add(sample_position_to_ms(buffer_start_sample));
            let leading_overlap_samples = boundary_overlap.len();
            let decode_samples = with_boundary_overlap(&boundary_overlap, &window);
            let result = decode_result(
                &mut decoder,
                decode_samples.as_ref(),
                window.len(),
                WindowResultKind::Committed,
                window_index,
                start_ms,
                (!committed_context.is_empty()).then_some(committed_context.as_str()),
                leading_overlap_samples,
            );
            update_committed_context(&mut committed_context, &result);
            if results_tx.send(result).is_err() {
                return;
            }
            buffer_start_sample = buffer_start_sample.saturating_add(window.len() as u64);
            update_boundary_overlap(&mut boundary_overlap, &window);
            window_index += 1;
            last_partial_len = 0;
            vad_scanned_len = 0;
        }
        // No commit moved the buffer's front. On the next input chunk only
        // re-check enough trailing frames to recognise a pause crossing this
        // boundary.
        vad_scanned_len = buffer.len();

        if buffer.len() >= PARTIAL_MIN_SAMPLES
            && buffer.len().saturating_sub(last_partial_len) >= PARTIAL_STEP_SAMPLES
            && contains_likely_speech(&buffer)
        {
            let start_ms =
                timeline_start_ms.saturating_add(sample_position_to_ms(buffer_start_sample));
            let leading_overlap_samples = boundary_overlap.len();
            let decode_samples = with_boundary_overlap(&boundary_overlap, &buffer);
            if !try_send_partial(
                &results_tx,
                decode_result(
                    &mut decoder,
                    decode_samples.as_ref(),
                    buffer.len(),
                    WindowResultKind::Partial,
                    window_index,
                    start_ms,
                    (!committed_context.is_empty()).then_some(committed_context.as_str()),
                    leading_overlap_samples,
                ),
            ) {
                return;
            }
            last_partial_len = buffer.len();
        }

        if chunk.captured_end_sample > expected_input_sample {
            let gap_start_sample = if !buffer.is_empty()
                && buffer.len() < FINAL_MIN_SAMPLES
                && contains_likely_speech(&buffer)
            {
                buffer_start_sample
            } else {
                expected_input_sample
            };
            if !flush_trailing_buffer(
                &mut decoder,
                &mut buffer,
                &results_tx,
                &mut window_index,
                timeline_start_ms,
                buffer_start_sample,
                &mut committed_context,
                &mut boundary_overlap,
            ) || !send_capture_gap(
                &results_tx,
                &mut window_index,
                timeline_start_ms,
                gap_start_sample,
                chunk.captured_end_sample,
            ) {
                return;
            }
            expected_input_sample = chunk.captured_end_sample;
            buffer_start_sample = expected_input_sample;
            last_partial_len = 0;
            vad_scanned_len = 0;
            boundary_overlap.clear();
        }
    }
}

fn with_boundary_overlap<'a>(overlap: &[f32], samples: &'a [f32]) -> std::borrow::Cow<'a, [f32]> {
    if overlap.is_empty() {
        return std::borrow::Cow::Borrowed(samples);
    }
    let mut joined = Vec::with_capacity(overlap.len() + samples.len());
    joined.extend_from_slice(overlap);
    joined.extend_from_slice(samples);
    std::borrow::Cow::Owned(joined)
}

fn update_boundary_overlap(overlap: &mut Vec<f32>, committed: &[f32]) {
    overlap.clear();
    if committed.len() < WINDOW_SAMPLES {
        return;
    }
    let keep = HARD_BOUNDARY_OVERLAP_SAMPLES.min(committed.len());
    overlap.extend_from_slice(&committed[committed.len() - keep..]);
}

fn sample_position_to_ms(sample: u64) -> u64 {
    sample.saturating_mul(1_000) / SAMPLE_RATE as u64
}

// The tail flush mutates the same state bundle as the main decode loop; keep
// those references explicit until the loop itself has an owned state type.
#[allow(clippy::too_many_arguments)]
fn flush_trailing_buffer<D: SessionDecoder>(
    decoder: &mut std::result::Result<D, String>,
    buffer: &mut Vec<f32>,
    results_tx: &ResultSender,
    window_index: &mut u64,
    timeline_start_ms: u64,
    buffer_start_sample: u64,
    committed_context: &mut String,
    boundary_overlap: &mut Vec<f32>,
) -> bool {
    let should_decode = buffer.len() >= FINAL_MIN_SAMPLES && contains_likely_speech(buffer);
    if should_decode {
        let start_ms = timeline_start_ms.saturating_add(sample_position_to_ms(buffer_start_sample));
        let leading_overlap_samples = boundary_overlap.len();
        let decode_samples = with_boundary_overlap(boundary_overlap, buffer);
        let result = decode_result(
            decoder,
            decode_samples.as_ref(),
            buffer.len(),
            WindowResultKind::Committed,
            *window_index,
            start_ms,
            (!committed_context.is_empty()).then_some(committed_context.as_str()),
            leading_overlap_samples,
        );
        update_committed_context(committed_context, &result);
        if results_tx.send(result).is_err() {
            return false;
        }
        *window_index = window_index.saturating_add(1);
    }
    buffer.clear();
    boundary_overlap.clear();
    true
}

fn update_committed_context(context: &mut String, result: &WindowResult) {
    let Ok(transcription) = result.outcome.as_ref() else {
        return;
    };
    let decoded = transcription
        .segments
        .iter()
        .map(|segment| segment.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if decoded.is_empty() {
        return;
    }
    if !context.is_empty() {
        context.push(' ');
    }
    context.push_str(&decoded);
    let character_count = context.chars().count();
    if character_count > CONTEXT_MAX_CHARS {
        *context = context
            .chars()
            .skip(character_count - CONTEXT_MAX_CHARS)
            .collect::<String>()
            .trim_start()
            .to_string();
    }
}

fn send_capture_gap(
    results_tx: &ResultSender,
    window_index: &mut u64,
    timeline_start_ms: u64,
    start_sample: u64,
    end_sample: u64,
) -> bool {
    if end_sample <= start_sample {
        return true;
    }
    let start_ms = timeline_start_ms.saturating_add(sample_position_to_ms(start_sample));
    let end_ms = timeline_start_ms
        .saturating_add(sample_position_to_ms(end_sample))
        .max(start_ms.saturating_add(1));
    let sent = results_tx
        .send(WindowResult {
            kind: WindowResultKind::Gap,
            window_index: *window_index,
            start_ms,
            end_ms,
            decode_ms: 0,
            outcome: Err(AppError::Capture(
                "Meeting transcript has a gap because live audio processing was overloaded."
                    .to_string(),
            )),
        })
        .is_ok();
    if sent {
        *window_index = window_index.saturating_add(1);
    }
    sent
}

#[cfg(test)]
mod tests;
