//! Rolling-window Streaming decode. See `docs/architecture.md`'s Streaming
//! Decode/Session Pipeline section for lifecycle and trade-off details.

use crate::audio::SAMPLE_RATE;
use crate::error::AppError;
use crate::streaming_audio::CapturedAudioChunk;
use crate::transcribe::{self, Transcription};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};
use whisper_rs::{WhisperContext, WhisperState};

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

/// A half-second is long enough to preserve a spoken trailing word without
/// decoding callback noise at shutdown.
const FINAL_MIN_SAMPLES: usize = SAMPLE_RATE as usize / 2;
const MEANINGFUL_AUDIO_RMS: f64 = 0.001;
const VAD_STEADY_NOISE_MAX_RMS: f64 = 0.012;
const VAD_STEADY_NOISE_MAX_RELATIVE_RANGE: f64 = 0.08;

/// At most this many decoded partial/committed results may wait for
/// persistence and renderer emission. A full queue backpressures the decoder;
/// the independently bounded audio queue then applies its drop-newest policy.
pub const RESULT_QUEUE_CAPACITY: usize = 16;

pub fn result_channel() -> (SyncSender<WindowResult>, Receiver<WindowResult>) {
    sync_channel(RESULT_QUEUE_CAPACITY)
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
        context: Option<&str>,
        profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        transcribe::transcribe_with_state_and_prompt_profile(
            &mut self.state,
            samples,
            context,
            profile,
        )
    }

    fn has_reliable_segment_timestamps(&self) -> bool {
        true
    }
}

/// Window decoder for the GGUF Qwen3-ASR runtime. llama.cpp/MTMD owns audio
/// encoding and language detection; the surrounding window supplies stable
/// session-relative timestamps.
#[cfg(target_os = "macos")]
pub struct QwenGgufSessionDecoder {
    model: std::sync::Arc<crate::qwen_gguf_asr::QwenGgufAsrModel>,
}

#[cfg(target_os = "macos")]
impl QwenGgufSessionDecoder {
    pub fn new(model: std::sync::Arc<crate::qwen_gguf_asr::QwenGgufAsrModel>) -> Self {
        Self { model }
    }
}

#[cfg(target_os = "macos")]
impl SessionDecoder for QwenGgufSessionDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        context: Option<&str>,
        _profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        self.model.transcribe_window_with_context(samples, context)
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
fn take_commit_window(buffer: &mut Vec<f32>) -> Option<Vec<f32>> {
    let scan_end = buffer.len().min(WINDOW_SAMPLES);
    if scan_end >= NATURAL_COMMIT_MIN_SAMPLES + NATURAL_PAUSE_SAMPLES {
        let mut quiet_start: Option<usize> = None;
        let mut quiet_threshold = VAD_SILENCE_RMS;
        for start in (NATURAL_COMMIT_MIN_SAMPLES..scan_end).step_by(VAD_FRAME_SAMPLES) {
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
    results_tx: SyncSender<WindowResult>,
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
    results_tx: SyncSender<WindowResult>,
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
        }

        while let Some(window) = take_commit_window(&mut buffer) {
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
        }

        if buffer.len() >= PARTIAL_MIN_SAMPLES
            && buffer.len().saturating_sub(last_partial_len) >= PARTIAL_STEP_SAMPLES
            && contains_likely_speech(&buffer)
        {
            let start_ms =
                timeline_start_ms.saturating_add(sample_position_to_ms(buffer_start_sample));
            let leading_overlap_samples = boundary_overlap.len();
            let decode_samples = with_boundary_overlap(&boundary_overlap, &buffer);
            if results_tx
                .send(decode_result(
                    &mut decoder,
                    decode_samples.as_ref(),
                    buffer.len(),
                    WindowResultKind::Partial,
                    window_index,
                    start_ms,
                    (!committed_context.is_empty()).then_some(committed_context.as_str()),
                    leading_overlap_samples,
                ))
                .is_err()
            {
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

fn flush_trailing_buffer<D: SessionDecoder>(
    decoder: &mut std::result::Result<D, String>,
    buffer: &mut Vec<f32>,
    results_tx: &SyncSender<WindowResult>,
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
    results_tx: &SyncSender<WindowResult>,
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
                "Streaming transcript has a gap because live audio processing was overloaded."
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
mod tests {
    use super::*;
    use std::sync::Arc;

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
    fn commit_window_moves_to_the_end_of_a_contiguous_quiet_pause() {
        let mut buffer = vec![0.05_f32; WINDOW_SAMPLES + 100];
        let quiet_start = WINDOW_SAMPLES - SAMPLE_RATE as usize / 2;
        let quiet_pause_samples = VAD_FRAME_SAMPLES * 5;
        buffer[quiet_start..quiet_start + quiet_pause_samples].fill(0.0);

        let window = take_commit_window(&mut buffer).expect("commit window");

        assert_eq!(window.len(), quiet_start + quiet_pause_samples);
        assert_eq!(buffer.len(), WINDOW_SAMPLES + 100 - window.len());
    }

    #[test]
    fn commit_window_ignores_one_quiet_frame_inside_continuous_speech() {
        let mut buffer = vec![0.05_f32; WINDOW_SAMPLES + 100];
        let transient_start = WINDOW_SAMPLES - SAMPLE_RATE as usize / 2;
        buffer[transient_start..transient_start + VAD_FRAME_SAMPLES].fill(0.0);

        let window = take_commit_window(&mut buffer).expect("commit window");

        assert_eq!(
            window.len(),
            WINDOW_SAMPLES,
            "one quiet 20 ms frame is not a speech pause and must not move the boundary"
        );
    }

    #[test]
    fn commit_window_ends_a_natural_utterance_before_the_hard_window_limit() {
        let speech_samples = SAMPLE_RATE as usize * 3;
        let pause_samples = SAMPLE_RATE as usize * 3 / 5;
        let mut buffer = vec![0.05_f32; speech_samples];
        buffer.extend(vec![0.0_f32; pause_samples]);

        let window = take_commit_window(&mut buffer).expect("natural pause commits utterance");

        assert_eq!(window.len(), speech_samples + pause_samples);
        assert!(buffer.is_empty());
        assert!(window.len() < WINDOW_SAMPLES);
    }

    #[test]
    fn commit_window_adapts_to_a_steady_background_noise_floor() {
        let speech_samples = SAMPLE_RATE as usize * 3;
        let pause_samples = NATURAL_PAUSE_SAMPLES;
        let mut buffer = vec![0.04_f32; speech_samples];
        buffer.extend(vec![0.006_f32; pause_samples]);

        let window = take_commit_window(&mut buffer).expect("background-relative pause commits");

        assert_eq!(window.len(), speech_samples + pause_samples);
        assert!(buffer.is_empty());
    }

    #[test]
    fn commit_window_adapts_after_a_long_utterance() {
        let speech_samples = SAMPLE_RATE as usize * 12;
        let pause_samples = NATURAL_PAUSE_SAMPLES;
        let mut buffer = vec![0.04_f32; speech_samples];
        buffer.extend(vec![0.006_f32; pause_samples]);

        let window = take_commit_window(&mut buffer)
            .expect("a noisy pause must still commit after a long utterance");

        assert_eq!(window.len(), speech_samples + pause_samples);
        assert!(buffer.is_empty());
    }

    #[test]
    fn steady_background_noise_without_speech_is_not_a_natural_commit() {
        let mut buffer = vec![0.006_f32; SAMPLE_RATE as usize * 4];

        assert!(take_commit_window(&mut buffer).is_none());
        assert_eq!(buffer.len(), SAMPLE_RATE as usize * 4);
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
    fn recorder_claim_remains_observable_until_pipeline_release() {
        let state = AtomicU8::new(IDLE);
        try_claim_recorder(&state).expect("Recorder claim");

        assert_eq!(current_whisper_user(&state), Some(WhisperUser::Recorder));

        release_whisper_busy(&state);
        assert_eq!(current_whisper_user(&state), None);
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

    #[test]
    fn decoded_result_queue_has_a_hard_capacity() {
        let (tx, _rx) = result_channel();
        let result = |window_index| WindowResult {
            kind: WindowResultKind::Partial,
            window_index,
            start_ms: 0,
            end_ms: 1,
            decode_ms: 0,
            outcome: Ok(Transcription {
                segments: Vec::new(),
                language: "en".to_string(),
            }),
        };

        for window_index in 0..RESULT_QUEUE_CAPACITY as u64 {
            tx.try_send(result(window_index))
                .expect("capacity slots accept decoded results");
        }
        assert!(matches!(
            tx.try_send(result(RESULT_QUEUE_CAPACITY as u64)),
            Err(std::sync::mpsc::TrySendError::Full(_))
        ));
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
            _context: Option<&str>,
            _profile: SessionDecodeProfile,
        ) -> crate::error::Result<Transcription> {
            self.decoded.push(samples.len());
            Ok(Transcription {
                segments: vec![],
                language: "en".to_string(),
            })
        }
    }

    fn two_windows_channel() -> (
        std::sync::mpsc::Sender<CapturedAudioChunk>,
        std::sync::mpsc::Receiver<CapturedAudioChunk>,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(captured_chunk(0, vec![0.05_f32; WINDOW_SAMPLES * 2]))
            .expect("send two windows of samples");
        (tx, rx)
    }

    fn captured_chunk(start_sample: u64, samples: Vec<f32>) -> CapturedAudioChunk {
        CapturedAudioChunk {
            start_sample,
            captured_end_sample: start_sample.saturating_add(samples.len() as u64),
            samples,
        }
    }

    #[test]
    fn decode_loop_creates_one_decoder_for_a_multi_window_session() {
        let creations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (samples_tx, samples_rx) = two_windows_channel();
        let (results_tx, results_rx) = result_channel();
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
        let (results_tx, results_rx) = result_channel();
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

    // WP-113 S-1: disconnect is the Stop boundary. A meaningful suffix must
    // be decoded once even when it never reaches the regular hard cap.
    #[test]
    fn disconnect_flushes_meaningful_trailing_audio_exactly_once() {
        let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        let trailing_samples = SAMPLE_RATE as usize / 2;
        samples_tx
            .send(captured_chunk(0, vec![0.05_f32; trailing_samples]))
            .expect("send a meaningful trailing buffer");
        drop(samples_tx);
        let (results_tx, results_rx) = result_channel();

        let decoded = Arc::clone(&decoded_lengths);
        run_windowed_decode(
            move || {
                Ok(RecordingDecoder {
                    decoded_lengths: decoded,
                })
            },
            samples_rx,
            results_tx,
            0,
        );

        let results: Vec<_> = results_rx.try_iter().collect();
        assert!(
            results.len() == 1,
            "Stop must commit one meaningful trailing decode, got {} results",
            results.len()
        );
        assert_eq!(results[0].kind, WindowResultKind::Committed);
        assert_eq!(
            results[0].end_ms, 500,
            "the trailing end timestamp must come from its 8,000 samples, not the 7s window size"
        );
        assert_eq!(
            *decoded_lengths.lock().expect("decoded-length mutex"),
            vec![trailing_samples],
            "the trailing samples must reach Whisper exactly once"
        );
    }

    // WP-113 failure partition: a final buffer can be non-empty without
    // containing meaningful speech. Silence and sub-threshold noise must not
    // invoke Whisper or create a committed result.
    #[test]
    fn disconnect_does_not_decode_silence_or_below_threshold_noise() {
        for samples in [
            vec![0.0_f32; SAMPLE_RATE as usize / 2],
            (0..SAMPLE_RATE as usize / 2)
                .map(|index| if index % 2 == 0 { 0.000_01 } else { -0.000_01 })
                .collect(),
        ] {
            let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
            let (samples_tx, samples_rx) = std::sync::mpsc::channel();
            samples_tx
                .send(captured_chunk(0, samples))
                .expect("send trailing noise");
            drop(samples_tx);
            let (results_tx, results_rx) = result_channel();

            let decoded = Arc::clone(&decoded_lengths);
            run_windowed_decode(
                move || {
                    Ok(RecordingDecoder {
                        decoded_lengths: decoded,
                    })
                },
                samples_rx,
                results_tx,
                0,
            );

            assert!(results_rx.try_iter().next().is_none());
            assert!(
                decoded_lengths
                    .lock()
                    .expect("decoded-length mutex")
                    .is_empty(),
                "silence/noise below the meaningful-audio predicate must be skipped"
            );
        }
    }

    #[test]
    fn steady_room_noise_never_emits_a_partial_or_hard_commit() {
        let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        samples_tx
            .send(captured_chunk(
                0,
                vec![0.006_f32; WINDOW_SAMPLES + SAMPLE_RATE as usize],
            ))
            .expect("send steady room noise past the hard boundary");
        drop(samples_tx);
        let (results_tx, results_rx) = result_channel();

        let decoded = Arc::clone(&decoded_lengths);
        run_windowed_decode(
            move || {
                Ok(RecordingDecoder {
                    decoded_lengths: decoded,
                })
            },
            samples_rx,
            results_tx,
            0,
        );

        assert!(results_rx.try_iter().next().is_none());
        assert!(
            decoded_lengths.lock().unwrap().is_empty(),
            "steady background noise must not invoke the ASR model"
        );
    }

    #[test]
    fn varying_low_gain_speech_reaches_the_decoder() {
        let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        let samples = (0..WINDOW_SAMPLES)
            .map(|index| {
                if (index / VAD_FRAME_SAMPLES) % 2 == 0 {
                    0.003_f32
                } else {
                    0.006_f32
                }
            })
            .collect();
        samples_tx
            .send(captured_chunk(0, samples))
            .expect("send low-gain speech-like signal");
        drop(samples_tx);
        let (results_tx, results_rx) = result_channel();

        let decoded = Arc::clone(&decoded_lengths);
        run_windowed_decode(
            move || {
                Ok(RecordingDecoder {
                    decoded_lengths: decoded,
                })
            },
            samples_rx,
            results_tx,
            0,
        );

        assert!(results_rx.try_iter().next().is_some());
        assert!(
            !decoded_lengths.lock().unwrap().is_empty(),
            "ambiguous low-gain audio must fail safe toward ASR, not silent deletion"
        );
    }

    #[test]
    fn capture_gaps_are_explicit_and_keep_the_original_sample_timeline() {
        let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        samples_tx
            .send(captured_chunk(0, vec![0.05; SAMPLE_RATE as usize]))
            .expect("first second");
        samples_tx
            .send(captured_chunk(
                SAMPLE_RATE as u64 * 2,
                vec![0.05; SAMPLE_RATE as usize],
            ))
            .expect("third second after one-second overload gap");
        drop(samples_tx);
        let (results_tx, results_rx) = result_channel();

        let decoded = Arc::clone(&decoded_lengths);
        run_windowed_decode(
            move || {
                Ok(RecordingDecoder {
                    decoded_lengths: decoded,
                })
            },
            samples_rx,
            results_tx,
            0,
        );

        let results: Vec<_> = results_rx.try_iter().collect();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].kind, WindowResultKind::Committed);
        assert_eq!((results[0].start_ms, results[0].end_ms), (0, 1_000));
        assert_eq!(results[1].kind, WindowResultKind::Gap);
        assert_eq!((results[1].start_ms, results[1].end_ms), (1_000, 2_000));
        assert!(results[1].outcome.is_err());
        assert_eq!(results[2].kind, WindowResultKind::Committed);
        assert_eq!((results[2].start_ms, results[2].end_ms), (2_000, 3_000));
    }

    #[test]
    fn meaningful_short_prefix_before_capture_gap_is_included_in_failed_gap() {
        let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        let prefix_samples = SAMPLE_RATE as usize / 4;
        samples_tx
            .send(captured_chunk(0, vec![0.05; prefix_samples]))
            .expect("meaningful 250 ms prefix");
        samples_tx
            .send(captured_chunk(
                SAMPLE_RATE as u64,
                vec![0.05; SAMPLE_RATE as usize / 2],
            ))
            .expect("audio after the capture gap");
        drop(samples_tx);
        let (results_tx, results_rx) = result_channel();

        let decoded = Arc::clone(&decoded_lengths);
        run_windowed_decode(
            move || {
                Ok(RecordingDecoder {
                    decoded_lengths: decoded,
                })
            },
            samples_rx,
            results_tx,
            0,
        );

        let results: Vec<_> = results_rx.try_iter().collect();
        let gap = results
            .iter()
            .find(|result| result.kind == WindowResultKind::Gap)
            .expect("the overloaded interval must be explicit");
        assert_eq!(
            (gap.start_ms, gap.end_ms),
            (0, 1_000),
            "a meaningful prefix too short to decode must be folded into the failed gap, not dropped"
        );
        assert!(gap.outcome.is_err());
        assert_eq!(
            *decoded_lengths.lock().expect("decoded-length mutex"),
            vec![SAMPLE_RATE as usize / 2],
            "the short prefix is represented by the failed gap rather than sent to Whisper"
        );
    }

    // WP-113 reconciliation contract: local rolling decodes expose their
    // mutability instead of making consumers infer it. Revisions share the
    // same unstable boundary, while an earlier committed result never moves.
    #[test]
    fn local_partials_can_revise_without_moving_committed_boundaries() {
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        samples_tx
            .send(captured_chunk(0, vec![0.05_f32; WINDOW_SAMPLES]))
            .expect("send first committed window");
        samples_tx
            .send(captured_chunk(
                WINDOW_SAMPLES as u64,
                vec![0.05_f32; PARTIAL_MIN_SAMPLES],
            ))
            .expect("send initial partial");
        samples_tx
            .send(captured_chunk(
                (WINDOW_SAMPLES + PARTIAL_MIN_SAMPLES) as u64,
                vec![0.05_f32; PARTIAL_STEP_SAMPLES],
            ))
            .expect("send partial revision");
        samples_tx
            .send(captured_chunk(
                (WINDOW_SAMPLES + PARTIAL_MIN_SAMPLES + PARTIAL_STEP_SAMPLES) as u64,
                vec![0.05_f32; WINDOW_SAMPLES - PARTIAL_MIN_SAMPLES - PARTIAL_STEP_SAMPLES],
            ))
            .expect("complete second window");
        drop(samples_tx);
        let (results_tx, results_rx) = result_channel();

        run_windowed_decode(
            || Ok(RevisionDecoder { decode_index: 0 }),
            samples_rx,
            results_tx,
            0,
        );

        let results: Vec<_> = results_rx.try_iter().collect();
        assert_eq!(results.len(), 4, "one commit, two revisions, one commit");
        assert_eq!(results[0].kind, WindowResultKind::Committed);
        assert_eq!((results[0].start_ms, results[0].end_ms), (0, WINDOW_MS));
        assert_eq!(results[1].kind, WindowResultKind::Partial);
        assert_eq!(results[2].kind, WindowResultKind::Partial);
        assert_eq!(results[1].start_ms, WINDOW_MS);
        assert_eq!(results[2].start_ms, WINDOW_MS);
        assert_ne!(result_text(&results[1]), result_text(&results[2]));
        assert_eq!(results[3].kind, WindowResultKind::Committed);
        assert_eq!(
            (results[3].start_ms, results[3].end_ms),
            (WINDOW_MS, WINDOW_MS * 2)
        );
        assert_eq!(
            (results[0].start_ms, results[0].end_ms),
            (0, WINDOW_MS),
            "later partial revisions must not mutate a committed boundary"
        );
    }

    #[test]
    fn live_partials_use_fast_decode_and_commits_use_quality_decode() {
        let profiles = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        samples_tx
            .send(captured_chunk(0, vec![0.05_f32; PARTIAL_MIN_SAMPLES]))
            .unwrap();
        samples_tx
            .send(captured_chunk(
                PARTIAL_MIN_SAMPLES as u64,
                vec![0.05_f32; WINDOW_SAMPLES - PARTIAL_MIN_SAMPLES],
            ))
            .unwrap();
        drop(samples_tx);
        let (results_tx, _results_rx) = result_channel();
        let recorded = Arc::clone(&profiles);

        run_windowed_decode(
            move || Ok(ProfileRecordingDecoder { profiles: recorded }),
            samples_rx,
            results_tx,
            0,
        );

        assert_eq!(
            *profiles.lock().unwrap(),
            vec![
                SessionDecodeProfile::FastPartial,
                SessionDecodeProfile::Quality
            ]
        );
    }

    #[test]
    fn confirmed_text_is_context_for_the_next_audio_window() {
        let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        samples_tx
            .send(captured_chunk(0, vec![0.05_f32; WINDOW_SAMPLES]))
            .unwrap();
        samples_tx
            .send(captured_chunk(
                WINDOW_SAMPLES as u64,
                vec![0.05_f32; FINAL_MIN_SAMPLES],
            ))
            .unwrap();
        drop(samples_tx);
        let (results_tx, _results_rx) = result_channel();
        let recorded = Arc::clone(&contexts);

        run_windowed_decode(
            move || Ok(ContextRecordingDecoder { contexts: recorded }),
            samples_rx,
            results_tx,
            0,
        );

        assert_eq!(
            *contexts.lock().unwrap(),
            vec![None, Some("confirmed phrase".to_string())]
        );
    }

    // WP-113 balanced decoding: the audio overlap at a forced boundary can
    // make the model repeat the confirmed suffix at the start of the next
    // committed result. Reconciliation removes only that boundary copy; the
    // already-published prefix remains byte-for-byte stable.
    #[test]
    fn committed_boundary_overlap_is_removed_without_rewriting_confirmed_prefix() {
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        samples_tx
            .send(captured_chunk(0, vec![0.05_f32; WINDOW_SAMPLES * 2]))
            .unwrap();
        let (results_tx, results_rx) = result_channel();
        drop(samples_tx);

        run_windowed_decode(
            || Ok(TimestampedOverlapDecoder { decode_index: 0 }),
            samples_rx,
            results_tx,
            0,
        );

        let results: Vec<_> = results_rx.try_iter().collect();
        assert_eq!(result_text(&results[0]), "сегодня обсуждаем план релиза");
        assert_eq!(
            result_text(&results[1]),
            "на завтра",
            "the repeated suffix/prefix comes from boundary overlap and must not be published twice"
        );
    }

    #[test]
    fn hard_boundary_second_decode_includes_audio_from_previous_window() {
        let decoded = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        let samples = (0..WINDOW_SAMPLES * 2)
            .map(|index| 0.02_f32 + index as f32 / (WINDOW_SAMPLES * 4) as f32)
            .collect::<Vec<_>>();
        let first_non_overlap_sample = samples[WINDOW_SAMPLES];
        samples_tx.send(captured_chunk(0, samples)).unwrap();
        drop(samples_tx);
        let (results_tx, results_rx) = result_channel();
        let recorded = Arc::clone(&decoded);

        run_windowed_decode(
            move || Ok(BoundaryRecordingDecoder { decoded: recorded }),
            samples_rx,
            results_tx,
            0,
        );

        let results: Vec<_> = results_rx.try_iter().collect();
        let decoded = decoded.lock().unwrap();
        assert_eq!(results.len(), 2, "overlap must not create a third window");
        assert_eq!(decoded.len(), 2);
        assert!(
            decoded[1].0 > WINDOW_SAMPLES,
            "the second decode must include a short tail from the first logical window"
        );
        assert!(
            decoded[1].1 < first_non_overlap_sample,
            "the second decode must start before the non-overlapping hard boundary"
        );
    }

    // A full next utterance that happens to repeat the preceding phrase is
    // real speech, not a partial suffix/prefix overlap. Do not erase it.
    #[test]
    fn genuine_adjacent_repeated_phrase_is_not_removed_as_boundary_overlap() {
        let (samples_tx, samples_rx) = two_windows_channel();
        let (results_tx, results_rx) = result_channel();
        drop(samples_tx);

        run_windowed_decode(
            || Ok(SequenceDecoder::new(["давайте начнем", "давайте начнем"])),
            samples_rx,
            results_tx,
            0,
        );

        let results: Vec<_> = results_rx.try_iter().collect();
        assert_eq!(result_text(&results[0]), "давайте начнем");
        assert_eq!(result_text(&results[1]), "давайте начнем");
    }

    #[test]
    fn genuine_repeated_prefix_with_a_continuation_is_not_removed() {
        let (samples_tx, samples_rx) = two_windows_channel();
        let (results_tx, results_rx) = result_channel();
        drop(samples_tx);

        run_windowed_decode(
            || {
                Ok(SequenceDecoder::new([
                    "сегодня обсуждаем план релиза",
                    "план релиза переносим на завтра",
                ]))
            },
            samples_rx,
            results_tx,
            0,
        );

        let results: Vec<_> = results_rx.try_iter().collect();
        assert_eq!(result_text(&results[0]), "сегодня обсуждаем план релиза");
        assert_eq!(
            result_text(&results[1]),
            "план релиза переносим на завтра",
            "text alone cannot prove that a repeated phrase came from overlap audio"
        );
    }

    struct ContextRecordingDecoder {
        contexts: Arc<std::sync::Mutex<Vec<Option<String>>>>,
    }

    struct ProfileRecordingDecoder {
        profiles: Arc<std::sync::Mutex<Vec<SessionDecodeProfile>>>,
    }

    impl SessionDecoder for ProfileRecordingDecoder {
        fn decode_window(
            &mut self,
            samples: &[f32],
            _context: Option<&str>,
            profile: SessionDecodeProfile,
        ) -> crate::error::Result<Transcription> {
            self.profiles.lock().unwrap().push(profile);
            Ok(Transcription {
                segments: vec![crate::transcribe::Segment {
                    start_ms: 0,
                    end_ms: samples_to_ms(samples.len()),
                    text: "decoded".into(),
                    speaker_id: None,
                }],
                language: "en".into(),
            })
        }
    }

    impl SessionDecoder for ContextRecordingDecoder {
        fn decode_window(
            &mut self,
            samples: &[f32],
            context: Option<&str>,
            _profile: SessionDecodeProfile,
        ) -> crate::error::Result<Transcription> {
            self.contexts
                .lock()
                .unwrap()
                .push(context.map(str::to_string));
            Ok(Transcription {
                segments: vec![crate::transcribe::Segment {
                    start_ms: 0,
                    end_ms: samples_to_ms(samples.len()),
                    text: "confirmed phrase".into(),
                    speaker_id: None,
                }],
                language: "en".into(),
            })
        }
    }

    struct RecordingDecoder {
        decoded_lengths: Arc<std::sync::Mutex<Vec<usize>>>,
    }

    struct SequenceDecoder {
        texts: std::collections::VecDeque<String>,
    }

    struct TimestampedOverlapDecoder {
        decode_index: usize,
    }

    struct BoundaryRecordingDecoder {
        decoded: Arc<std::sync::Mutex<Vec<(usize, f32)>>>,
    }

    impl SessionDecoder for BoundaryRecordingDecoder {
        fn decode_window(
            &mut self,
            samples: &[f32],
            _context: Option<&str>,
            _profile: SessionDecodeProfile,
        ) -> crate::error::Result<Transcription> {
            self.decoded
                .lock()
                .unwrap()
                .push((samples.len(), samples[0]));
            Ok(Transcription {
                segments: vec![],
                language: "en".to_string(),
            })
        }
    }

    impl SequenceDecoder {
        fn new<const N: usize>(texts: [&str; N]) -> Self {
            Self {
                texts: texts.into_iter().map(str::to_string).collect(),
            }
        }
    }

    impl SessionDecoder for SequenceDecoder {
        fn decode_window(
            &mut self,
            samples: &[f32],
            _context: Option<&str>,
            _profile: SessionDecodeProfile,
        ) -> crate::error::Result<Transcription> {
            Ok(Transcription {
                segments: vec![crate::transcribe::Segment {
                    start_ms: 0,
                    end_ms: samples_to_ms(samples.len()),
                    text: self.texts.pop_front().expect("one result per window"),
                    speaker_id: None,
                }],
                language: "ru".to_string(),
            })
        }
    }

    impl SessionDecoder for TimestampedOverlapDecoder {
        fn decode_window(
            &mut self,
            samples: &[f32],
            _context: Option<&str>,
            _profile: SessionDecodeProfile,
        ) -> crate::error::Result<Transcription> {
            let segments = if self.decode_index == 0 {
                vec![crate::transcribe::Segment {
                    start_ms: 0,
                    end_ms: samples_to_ms(samples.len()),
                    text: "сегодня обсуждаем план релиза".to_string(),
                    speaker_id: None,
                }]
            } else {
                vec![
                    crate::transcribe::Segment {
                        start_ms: 0,
                        end_ms: 700,
                        text: "план релиза".to_string(),
                        speaker_id: None,
                    },
                    crate::transcribe::Segment {
                        start_ms: 700,
                        end_ms: samples_to_ms(samples.len()),
                        text: "на завтра".to_string(),
                        speaker_id: None,
                    },
                ]
            };
            self.decode_index += 1;
            Ok(Transcription {
                segments,
                language: "ru".to_string(),
            })
        }

        fn has_reliable_segment_timestamps(&self) -> bool {
            true
        }
    }

    impl SessionDecoder for RecordingDecoder {
        fn decode_window(
            &mut self,
            samples: &[f32],
            _context: Option<&str>,
            _profile: SessionDecodeProfile,
        ) -> crate::error::Result<Transcription> {
            self.decoded_lengths
                .lock()
                .expect("decoded-length mutex")
                .push(samples.len());
            Ok(Transcription {
                segments: vec![],
                language: "en".to_string(),
            })
        }
    }

    struct RevisionDecoder {
        decode_index: usize,
    }

    impl SessionDecoder for RevisionDecoder {
        fn decode_window(
            &mut self,
            samples: &[f32],
            _context: Option<&str>,
            _profile: SessionDecodeProfile,
        ) -> crate::error::Result<Transcription> {
            let text = [
                "first commit",
                "draft suffix",
                "revised suffix",
                "second commit",
            ][self.decode_index];
            self.decode_index += 1;
            Ok(Transcription {
                segments: vec![crate::transcribe::Segment {
                    start_ms: 0,
                    end_ms: samples.len() as u64 * 1_000 / SAMPLE_RATE as u64,
                    text: text.to_string(),
                    speaker_id: None,
                }],
                language: "en".to_string(),
            })
        }
    }

    fn result_text(result: &WindowResult) -> &str {
        result
            .outcome
            .as_ref()
            .expect("test decoder succeeds")
            .segments
            .first()
            .expect("test decoder emits text")
            .text
            .as_str()
    }
}
