//! Microphone audio preparation for Recorder capture.
//!
//! Native input callbacks are downmixed to mono without changing the hardware
//! sample rate. Provider-specific consumers can then derive a band-limited
//! stream at the exact rate their ASR contract requires.

use crate::streaming_audio::CapturedAudioChunk;
use rubato::audioadapter::{Adapter, AdapterMut};
use rubato::{
    calculate_cutoff, Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};
use std::collections::VecDeque;
use std::fmt;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError};

const RESAMPLER_INPUT_CHUNK_FRAMES: usize = 1_024;
const RESAMPLER_SINC_LENGTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicrophoneAudioError(String);

impl fmt::Display for MicrophoneAudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MicrophoneAudioError {}

/// Stateful channel converter for one input device.
///
/// The output clock is the device's native sample clock. Keeping resampling
/// out of the CPAL callback preserves a recoverable master and keeps the
/// real-time callback bounded.
pub struct MicrophoneChunkNormalizer {
    channels: usize,
    emitted_samples: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicrophoneClockSpan {
    pub start_sample: u64,
    pub captured_end_sample: u64,
}

/// Sample conversion used by the allocation-reuse contract tests and by
/// non-CPAL callers. Native CPAL formats use the equivalent closure-based
/// path so every supported device format keeps CPAL's canonical conversion.
pub trait IntoMicrophoneF32: Copy {
    fn into_microphone_f32(self) -> f32;
}

impl IntoMicrophoneF32 for f32 {
    fn into_microphone_f32(self) -> f32 {
        self
    }
}

impl IntoMicrophoneF32 for i16 {
    fn into_microphone_f32(self) -> f32 {
        f32::from(self) / 32_768.0
    }
}

/// Owns one preallocated mono callback buffer and its native sample clock.
///
/// Production rotates the storage through a bounded recycle pool before a
/// chunk crosses the callback boundary. `prepare_from_with` never grows the
/// buffer: an unexpectedly large hardware callback becomes an explicit error
/// instead of allocating on CoreAudio's real-time thread.
pub struct ReusableMicrophoneChunkBuffer {
    channels: usize,
    frame_capacity: usize,
    samples: Vec<f32>,
    emitted_samples: u64,
}

impl ReusableMicrophoneChunkBuffer {
    pub fn new(channels: usize, frame_capacity: usize) -> Result<Self, MicrophoneAudioError> {
        if channels == 0 {
            return Err(MicrophoneAudioError(
                "microphone channel count must be greater than zero".into(),
            ));
        }
        if frame_capacity == 0 {
            return Err(MicrophoneAudioError(
                "microphone callback capacity must be greater than zero".into(),
            ));
        }
        Ok(Self {
            channels,
            frame_capacity,
            samples: Vec::with_capacity(frame_capacity),
            emitted_samples: 0,
        })
    }

    pub fn prepare_from<T: IntoMicrophoneF32>(
        &mut self,
        interleaved_samples: &[T],
    ) -> Result<MicrophoneClockSpan, MicrophoneAudioError> {
        self.prepare_from_with(interleaved_samples, IntoMicrophoneF32::into_microphone_f32)
    }

    fn prepare_from_with<T: Copy>(
        &mut self,
        interleaved_samples: &[T],
        convert: impl Fn(T) -> f32,
    ) -> Result<MicrophoneClockSpan, MicrophoneAudioError> {
        if interleaved_samples.len() % self.channels != 0 {
            return Err(MicrophoneAudioError(format!(
                "microphone callback returned {} samples for {} channels",
                interleaved_samples.len(),
                self.channels
            )));
        }
        let frames = interleaved_samples.len() / self.channels;
        if frames > self.frame_capacity {
            return Err(MicrophoneAudioError(format!(
                "microphone callback returned {frames} frames, exceeding the preallocated capacity of {}",
                self.frame_capacity
            )));
        }

        self.samples.clear();
        for frame in interleaved_samples.chunks_exact(self.channels) {
            let mono = frame.iter().copied().map(&convert).sum::<f32>() / self.channels as f32;
            self.samples.push(mono);
        }

        let start_sample = self.emitted_samples;
        self.emitted_samples = self.emitted_samples.saturating_add(frames as u64);
        Ok(MicrophoneClockSpan {
            start_sample,
            captured_end_sample: self.emitted_samples,
        })
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn capacity(&self) -> usize {
        self.samples.capacity()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicrophoneBufferPoolError {
    InvalidAudio(MicrophoneAudioError),
    Exhausted,
    Disconnected,
    InvalidStorage,
}

impl fmt::Display for MicrophoneBufferPoolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAudio(error) => error.fmt(formatter),
            Self::Exhausted => formatter.write_str("microphone callback buffer pool is exhausted"),
            Self::Disconnected => {
                formatter.write_str("microphone callback buffer pool is disconnected")
            }
            Self::InvalidStorage => {
                formatter.write_str("microphone callback buffer is smaller than its contract")
            }
        }
    }
}

impl std::error::Error for MicrophoneBufferPoolError {}

#[derive(Clone)]
struct MicrophoneBufferRecycler {
    tx: SyncSender<Vec<f32>>,
    minimum_capacity: usize,
}

impl MicrophoneBufferRecycler {
    fn recycle(&self, mut samples: Vec<f32>) {
        samples.clear();
        if samples.capacity() >= self.minimum_capacity {
            let _ = self.tx.try_send(samples);
        }
    }
}

/// A read-only native-rate mono chunk backed by a bounded allocation pool.
/// Dropping it returns valid storage to the producer without exposing a
/// mutable `Vec` to downstream consumers.
pub struct PooledMicrophoneSamples {
    span: MicrophoneClockSpan,
    samples: Option<Vec<f32>>,
    recycler: MicrophoneBufferRecycler,
}

impl PooledMicrophoneSamples {
    pub fn start_sample(&self) -> u64 {
        self.span.start_sample
    }

    pub fn captured_end_sample(&self) -> u64 {
        self.span.captured_end_sample
    }

    pub fn samples(&self) -> &[f32] {
        self.samples.as_deref().unwrap_or_default()
    }

    pub fn capacity(&self) -> usize {
        self.samples.as_ref().map_or(0, Vec::capacity)
    }
}

impl Drop for PooledMicrophoneSamples {
    fn drop(&mut self) {
        if let Some(samples) = self.samples.take() {
            self.recycler.recycle(samples);
        }
    }
}

/// Bounded owner of callback allocations and the native sample clock.
///
/// `prepare_from` atomically checks out one sufficiently-sized buffer, fills
/// it without allocation, and returns read-only ownership to the consumer.
pub struct ReusableMicrophoneBufferPool {
    channels: usize,
    frame_capacity: usize,
    available: Receiver<Vec<f32>>,
    recycler: MicrophoneBufferRecycler,
    emitted_samples: u64,
}

impl ReusableMicrophoneBufferPool {
    pub fn new(
        channels: usize,
        frame_capacity: usize,
        buffer_count: usize,
    ) -> Result<Self, MicrophoneAudioError> {
        if buffer_count == 0 {
            return Err(MicrophoneAudioError(
                "microphone callback buffer count must be greater than zero".into(),
            ));
        }
        let storage = (0..buffer_count)
            .map(|_| Vec::with_capacity(frame_capacity))
            .collect();
        Self::from_storage(channels, frame_capacity, storage)
    }

    pub fn from_storage(
        channels: usize,
        frame_capacity: usize,
        storage: Vec<Vec<f32>>,
    ) -> Result<Self, MicrophoneAudioError> {
        if channels == 0 {
            return Err(MicrophoneAudioError(
                "microphone channel count must be greater than zero".into(),
            ));
        }
        if frame_capacity == 0 {
            return Err(MicrophoneAudioError(
                "microphone callback capacity must be greater than zero".into(),
            ));
        }
        if storage.is_empty() {
            return Err(MicrophoneAudioError(
                "microphone callback buffer count must be greater than zero".into(),
            ));
        }

        let (tx, available) = sync_channel(storage.len());
        for mut samples in storage {
            samples.clear();
            tx.send(samples)
                .expect("the callback buffer receiver is alive during construction");
        }
        Ok(Self {
            channels,
            frame_capacity,
            available,
            recycler: MicrophoneBufferRecycler {
                tx,
                minimum_capacity: frame_capacity,
            },
            emitted_samples: 0,
        })
    }

    pub fn prepare_from<T: IntoMicrophoneF32>(
        &mut self,
        interleaved_samples: &[T],
    ) -> Result<PooledMicrophoneSamples, MicrophoneBufferPoolError> {
        self.prepare_from_with(interleaved_samples, IntoMicrophoneF32::into_microphone_f32)
    }

    fn prepare_from_with<T: Copy>(
        &mut self,
        interleaved_samples: &[T],
        convert: impl Fn(T) -> f32,
    ) -> Result<PooledMicrophoneSamples, MicrophoneBufferPoolError> {
        if interleaved_samples.len() % self.channels != 0 {
            return Err(MicrophoneBufferPoolError::InvalidAudio(
                MicrophoneAudioError(format!(
                    "microphone callback returned {} samples for {} channels",
                    interleaved_samples.len(),
                    self.channels
                )),
            ));
        }
        let frames = interleaved_samples.len() / self.channels;
        if frames > self.frame_capacity {
            return Err(MicrophoneBufferPoolError::InvalidAudio(
                MicrophoneAudioError(format!(
                    "microphone callback returned {frames} frames, exceeding the preallocated capacity of {}",
                    self.frame_capacity
                )),
            ));
        }

        let mut samples = match self.available.try_recv() {
            Ok(samples) => samples,
            Err(TryRecvError::Empty) => return Err(MicrophoneBufferPoolError::Exhausted),
            Err(TryRecvError::Disconnected) => return Err(MicrophoneBufferPoolError::Disconnected),
        };
        if samples.capacity() < self.frame_capacity {
            return Err(MicrophoneBufferPoolError::InvalidStorage);
        }

        samples.clear();
        for frame in interleaved_samples.chunks_exact(self.channels) {
            let mono = frame.iter().copied().map(&convert).sum::<f32>() / self.channels as f32;
            samples.push(mono);
        }

        let start_sample = self.emitted_samples;
        self.emitted_samples = self.emitted_samples.saturating_add(frames as u64);
        Ok(PooledMicrophoneSamples {
            span: MicrophoneClockSpan {
                start_sample,
                captured_end_sample: self.emitted_samples,
            },
            samples: Some(samples),
            recycler: self.recycler.clone(),
        })
    }
}

impl MicrophoneChunkNormalizer {
    pub fn new(source_sample_rate: u32, channels: usize) -> Result<Self, MicrophoneAudioError> {
        if source_sample_rate == 0 {
            return Err(MicrophoneAudioError(
                "microphone sample rate must be greater than zero".into(),
            ));
        }
        if channels == 0 {
            return Err(MicrophoneAudioError(
                "microphone channel count must be greater than zero".into(),
            ));
        }

        Ok(Self {
            channels,
            emitted_samples: 0,
        })
    }

    pub fn push_f32(
        &mut self,
        interleaved_samples: &[f32],
    ) -> Result<CapturedAudioChunk, MicrophoneAudioError> {
        if interleaved_samples.len() % self.channels != 0 {
            return Err(MicrophoneAudioError(format!(
                "microphone callback returned {} samples for {} channels",
                interleaved_samples.len(),
                self.channels
            )));
        }

        let start_sample = self.emitted_samples;
        let samples: Vec<f32> = interleaved_samples
            .chunks_exact(self.channels)
            .map(|frame| frame.iter().copied().sum::<f32>() / self.channels as f32)
            .collect();
        self.emitted_samples = self.emitted_samples.saturating_add(samples.len() as u64);
        Ok(CapturedAudioChunk {
            start_sample,
            captured_end_sample: self.emitted_samples,
            samples,
        })
    }
}

/// Stateful, streaming sinc resampler for provider-specific audio derivatives.
///
/// Arbitrary callback sizes are buffered into stable processing blocks so the
/// output is identical whether the source arrived in one slice or many. The
/// filter delay is removed and `finish` flushes the tail to the exact target
/// duration.
pub struct BandlimitedChunkResampler {
    source_sample_rate: u32,
    target_sample_rate: u32,
    engine: Option<Async<f32>>,
    pending: VecDeque<f32>,
    delay_remaining: usize,
    total_input_samples: u64,
    emitted_samples: u64,
    finished: bool,
}

impl BandlimitedChunkResampler {
    pub fn new(
        source_sample_rate: u32,
        target_sample_rate: u32,
    ) -> Result<Self, MicrophoneAudioError> {
        if source_sample_rate == 0 || target_sample_rate == 0 {
            return Err(MicrophoneAudioError(
                "resampler sample rates must be greater than zero".into(),
            ));
        }

        let engine = if source_sample_rate == target_sample_rate {
            None
        } else {
            let window = WindowFunction::Blackman2;
            let parameters = SincInterpolationParameters {
                sinc_len: RESAMPLER_SINC_LENGTH,
                f_cutoff: calculate_cutoff(RESAMPLER_SINC_LENGTH, window),
                oversampling_factor: 256,
                interpolation: SincInterpolationType::Quadratic,
                window,
            };
            let ratio = f64::from(target_sample_rate) / f64::from(source_sample_rate);
            Some(
                Async::<f32>::new_sinc(
                    ratio,
                    1.0,
                    &parameters,
                    RESAMPLER_INPUT_CHUNK_FRAMES,
                    1,
                    FixedAsync::Input,
                )
                .map_err(|error| {
                    MicrophoneAudioError(format!("failed to create audio resampler: {error}"))
                })?,
            )
        };
        let delay_remaining = engine.as_ref().map_or(0, Resampler::output_delay);

        Ok(Self {
            source_sample_rate,
            target_sample_rate,
            engine,
            pending: VecDeque::new(),
            delay_remaining,
            total_input_samples: 0,
            emitted_samples: 0,
            finished: false,
        })
    }

    pub fn push_f32(
        &mut self,
        samples: &[f32],
    ) -> Result<CapturedAudioChunk, MicrophoneAudioError> {
        if self.finished {
            return Err(MicrophoneAudioError(
                "cannot append audio after the resampler is finished".into(),
            ));
        }
        self.total_input_samples = self
            .total_input_samples
            .checked_add(samples.len() as u64)
            .ok_or_else(|| MicrophoneAudioError("resampler input clock overflowed".into()))?;

        if self.engine.is_none() {
            return Ok(self.emit(samples.to_vec()));
        }

        self.pending.extend(samples.iter().copied());
        let mut output = Vec::new();
        loop {
            let required = self
                .engine
                .as_ref()
                .expect("non-passthrough resampler has an engine")
                .input_frames_next();
            if self.pending.len() < required {
                break;
            }
            let input: Vec<f32> = self.pending.drain(..required).collect();
            let raw = process_resampler_block(
                self.engine
                    .as_mut()
                    .expect("non-passthrough resampler has an engine"),
                &input,
                None,
            )?;
            append_after_delay(&mut self.delay_remaining, raw, &mut output);
        }

        Ok(self.emit(output))
    }

    pub fn finish(&mut self) -> Result<CapturedAudioChunk, MicrophoneAudioError> {
        if self.finished {
            return Ok(self.emit(Vec::new()));
        }
        self.finished = true;

        if self.engine.is_none() {
            return Ok(self.emit(Vec::new()));
        }

        let expected_output_samples = div_ceil_u128(
            u128::from(self.total_input_samples) * u128::from(self.target_sample_rate),
            u128::from(self.source_sample_rate),
        ) as u64;
        let mut output = Vec::new();

        if !self.pending.is_empty() {
            let partial_len = self.pending.len();
            let required = self
                .engine
                .as_ref()
                .expect("non-passthrough resampler has an engine")
                .input_frames_next();
            let mut input = vec![0.0; required];
            for (slot, sample) in input.iter_mut().zip(self.pending.drain(..)) {
                *slot = sample;
            }
            let raw = process_resampler_block(
                self.engine
                    .as_mut()
                    .expect("non-passthrough resampler has an engine"),
                &input,
                Some(partial_len),
            )?;
            append_after_delay(&mut self.delay_remaining, raw, &mut output);
        }

        while self.emitted_samples.saturating_add(output.len() as u64) < expected_output_samples {
            let required = self
                .engine
                .as_ref()
                .expect("non-passthrough resampler has an engine")
                .input_frames_next();
            let input = vec![0.0; required];
            let raw = process_resampler_block(
                self.engine
                    .as_mut()
                    .expect("non-passthrough resampler has an engine"),
                &input,
                Some(0),
            )?;
            append_after_delay(&mut self.delay_remaining, raw, &mut output);
        }

        let remaining = expected_output_samples.saturating_sub(self.emitted_samples) as usize;
        output.truncate(remaining);
        Ok(self.emit(output))
    }

    fn emit(&mut self, samples: Vec<f32>) -> CapturedAudioChunk {
        let start_sample = self.emitted_samples;
        self.emitted_samples = self.emitted_samples.saturating_add(samples.len() as u64);
        CapturedAudioChunk {
            start_sample,
            captured_end_sample: self.emitted_samples,
            samples,
        }
    }
}

fn div_ceil_u128(numerator: u128, denominator: u128) -> u128 {
    numerator / denominator + u128::from(numerator % denominator != 0)
}

fn append_after_delay(delay_remaining: &mut usize, raw: Vec<f32>, output: &mut Vec<f32>) {
    let trim = (*delay_remaining).min(raw.len());
    *delay_remaining -= trim;
    output.extend_from_slice(&raw[trim..]);
}

fn process_resampler_block(
    resampler: &mut Async<f32>,
    input: &[f32],
    partial_len: Option<usize>,
) -> Result<Vec<f32>, MicrophoneAudioError> {
    let mut output = vec![0.0; resampler.output_frames_max()];
    let input_adapter = MonoSlice { samples: input };
    let mut output_adapter = MonoSliceMut {
        samples: &mut output,
    };
    let indexing = Indexing {
        input_offset: 0,
        output_offset: 0,
        partial_len,
        active_channels_mask: None,
    };
    let (_, written) = resampler
        .process_into_buffer(&input_adapter, &mut output_adapter, Some(&indexing))
        .map_err(|error| MicrophoneAudioError(format!("audio resampling failed: {error}")))?;
    output.truncate(written);
    Ok(output)
}

struct MonoSlice<'a> {
    samples: &'a [f32],
}

impl<'a> Adapter<'a, f32> for MonoSlice<'a> {
    unsafe fn read_sample_unchecked(&self, _channel: usize, frame: usize) -> f32 {
        unsafe { *self.samples.get_unchecked(frame) }
    }

    fn channels(&self) -> usize {
        1
    }

    fn frames(&self) -> usize {
        self.samples.len()
    }
}

struct MonoSliceMut<'a> {
    samples: &'a mut [f32],
}

impl<'a> Adapter<'a, f32> for MonoSliceMut<'a> {
    unsafe fn read_sample_unchecked(&self, _channel: usize, frame: usize) -> f32 {
        unsafe { *self.samples.get_unchecked(frame) }
    }

    fn channels(&self) -> usize {
        1
    }

    fn frames(&self) -> usize {
        self.samples.len()
    }
}

impl<'a> AdapterMut<'a, f32> for MonoSliceMut<'a> {
    unsafe fn write_sample_unchecked(
        &mut self,
        _channel: usize,
        frame: usize,
        value: &f32,
    ) -> bool {
        unsafe { *self.samples.get_unchecked_mut(frame) = *value };
        false
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use crate::error::{AppError, Result};
    use crate::microphone_permission::{self, MicrophonePermissionStatus};
    use crate::streaming_audio::QueueSendOutcome;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, Sample, SampleFormat, SizedSample};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
    use std::sync::Arc;

    const ERROR_QUEUE_CAPACITY: usize = 4;
    const CALLBACK_FRAME_CAPACITY: usize = 8_192;
    const CALLBACK_BUFFER_POOL_SIZE: usize = 33;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MicrophoneCaptureFailureKind {
        DeviceUnavailable,
        UnsupportedFormat,
        InputInterrupted,
        CaptureOverloaded,
        DownstreamDisconnected,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MicrophoneCaptureFailure {
        pub kind: MicrophoneCaptureFailureKind,
        pub message: String,
    }

    /// Maps bounded-queue delivery into Recorder lifecycle failures without
    /// touching a device, keeping overload/disconnect behavior testable.
    pub fn microphone_delivery_failure(
        outcome: QueueSendOutcome,
    ) -> Option<MicrophoneCaptureFailure> {
        match outcome {
            QueueSendOutcome::Sent => None,
            QueueSendOutcome::DroppedNewest => Some(MicrophoneCaptureFailure {
                kind: MicrophoneCaptureFailureKind::CaptureOverloaded,
                message: "Recorder audio processing fell behind; recording stopped to avoid silent data loss"
                    .into(),
            }),
            QueueSendOutcome::Disconnected => Some(MicrophoneCaptureFailure {
                kind: MicrophoneCaptureFailureKind::DownstreamDisconnected,
                message: "Recorder audio consumer disconnected; recording stopped to avoid invisible capture"
                    .into(),
            }),
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MicrophoneCaptureInfo {
        pub device_name: String,
        pub source_sample_rate: u32,
        pub source_channels: u16,
        pub sample_rate: u32,
    }

    pub struct MicrophoneCaptureStart {
        pub session: MicrophoneCaptureSession,
        pub failures: Receiver<MicrophoneCaptureFailure>,
        pub info: MicrophoneCaptureInfo,
    }

    /// Owns the native stream. Dropping this value synchronously releases the
    /// input device; CPAL's callback never waits for ASR or persistence.
    pub struct MicrophoneCaptureSession {
        _stream: cpal::Stream,
    }

    impl MicrophoneCaptureSession {
        /// Side-effect-free input preflight. This resolves the same default
        /// device/configuration used by `start` without opening a stream, so
        /// Recorder can validate permission, model and device before creating
        /// a durable session or audio artifact.
        pub fn probe_default_input() -> Result<MicrophoneCaptureInfo> {
            require_authorized_permission()?;
            let host = cpal::default_host();
            let device = host.default_input_device().ok_or_else(|| {
                AppError::Capture(
                    "no microphone input device is available; connect one and retry".into(),
                )
            })?;
            capture_info(&device)
        }

        /// Opens the current system-default input only after the explicit
        /// Recorder permission request returned `authorized`. Capture itself
        /// fails closed for every other status and never presents TCC.
        pub fn start(
            samples_tx: SyncSender<PooledMicrophoneSamples>,
        ) -> Result<MicrophoneCaptureStart> {
            require_authorized_permission()?;
            let host = cpal::default_host();
            let device = host.default_input_device().ok_or_else(|| {
                AppError::Capture(
                    "no microphone input device is available; connect one and retry".into(),
                )
            })?;
            let info = capture_info(&device)?;
            let supported_config = device.default_input_config().map_err(|error| {
                AppError::Capture(format!(
                    "the default microphone format is unavailable: {error}"
                ))
            })?;
            let sample_format = supported_config.sample_format();
            let config = supported_config.config();
            let (failure_tx, failures) = sync_channel(ERROR_QUEUE_CAPACITY);
            let failure_reported = Arc::new(AtomicBool::new(false));

            let stream = match sample_format {
                SampleFormat::I8 => build_input_stream::<i8>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                SampleFormat::I16 => build_input_stream::<i16>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                SampleFormat::I24 => build_input_stream::<cpal::I24>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                SampleFormat::I32 => build_input_stream::<i32>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                SampleFormat::I64 => build_input_stream::<i64>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                SampleFormat::U8 => build_input_stream::<u8>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                SampleFormat::U16 => build_input_stream::<u16>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                SampleFormat::U24 => build_input_stream::<cpal::U24>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                SampleFormat::U32 => build_input_stream::<u32>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                SampleFormat::U64 => build_input_stream::<u64>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                SampleFormat::F32 => build_input_stream::<f32>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                SampleFormat::F64 => build_input_stream::<f64>(
                    &device,
                    &config,
                    samples_tx,
                    failure_tx,
                    failure_reported,
                ),
                format => {
                    return Err(AppError::Capture(format!(
                        "microphone sample format {format} is not supported; select another input device"
                    )))
                }
            }
            .map_err(|error| {
                AppError::Capture(format!("failed to open the microphone input: {error}"))
            })?;

            stream.play().map_err(|error| {
                AppError::Capture(format!("failed to start microphone capture: {error}"))
            })?;

            Ok(MicrophoneCaptureStart {
                session: MicrophoneCaptureSession { _stream: stream },
                failures,
                info,
            })
        }
    }

    fn require_authorized_permission() -> Result<()> {
        match microphone_permission::get_microphone_permission_status() {
            MicrophonePermissionStatus::Authorized => Ok(()),
            MicrophonePermissionStatus::NotDetermined => Err(AppError::Capture(
                "microphone access has not been requested; start Recorder from an explicit user action and approve access"
                    .into(),
            )),
            MicrophonePermissionStatus::Denied => Err(AppError::Capture(
                "microphone access is denied; enable WhisperPilot in System Settings > Privacy & Security > Microphone"
                    .into(),
            )),
            MicrophonePermissionStatus::Restricted => Err(AppError::Capture(
                "microphone access is restricted by macOS policy".into(),
            )),
            MicrophonePermissionStatus::Unavailable => Err(AppError::Capture(
                "microphone permission status is unavailable on this system".into(),
            )),
        }
    }

    fn capture_info(device: &cpal::Device) -> Result<MicrophoneCaptureInfo> {
        let device_name = device
            .description()
            .map(|description| description.name().to_string())
            .unwrap_or_else(|_| "Default microphone".to_string());
        let config = device.default_input_config().map_err(|error| {
            AppError::Capture(format!(
                "the default microphone format is unavailable: {error}"
            ))
        })?;
        Ok(MicrophoneCaptureInfo {
            device_name,
            source_sample_rate: config.sample_rate(),
            source_channels: config.channels(),
            sample_rate: config.sample_rate(),
        })
    }

    fn build_input_stream<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        samples_tx: SyncSender<PooledMicrophoneSamples>,
        failure_tx: SyncSender<MicrophoneCaptureFailure>,
        failure_reported: Arc<AtomicBool>,
    ) -> std::result::Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: Sample + SizedSample,
        f32: FromSample<T>,
    {
        let channels = usize::from(config.channels);
        let mut callback_buffers = ReusableMicrophoneBufferPool::new(
            channels,
            CALLBACK_FRAME_CAPACITY,
            CALLBACK_BUFFER_POOL_SIZE,
        )
        .expect("CPAL returned a non-zero channel count and the pool is non-empty");
        let data_failure_tx = failure_tx.clone();
        let data_failure_reported = Arc::clone(&failure_reported);
        device.build_input_stream::<T, _, _>(
            config,
            move |input, _| {
                if data_failure_reported.load(Ordering::Acquire) {
                    return;
                }
                match callback_buffers.prepare_from_with(input, f32::from_sample) {
                    Ok(chunk) if chunk.samples().is_empty() => {}
                    Ok(chunk) => {
                        let outcome = match samples_tx.try_send(chunk) {
                            Ok(()) => QueueSendOutcome::Sent,
                            Err(TrySendError::Full(_)) => QueueSendOutcome::DroppedNewest,
                            Err(TrySendError::Disconnected(_)) => QueueSendOutcome::Disconnected,
                        };
                        if let Some(failure) = microphone_delivery_failure(outcome) {
                            report_failure_once(
                                &data_failure_tx,
                                &data_failure_reported,
                                failure.kind,
                                &failure.message,
                            );
                        }
                    }
                    Err(MicrophoneBufferPoolError::Exhausted) => report_failure_once(
                        &data_failure_tx,
                        &data_failure_reported,
                        MicrophoneCaptureFailureKind::CaptureOverloaded,
                        "Recorder callback buffer pool was exhausted; recording stopped to avoid silent data loss",
                    ),
                    Err(MicrophoneBufferPoolError::Disconnected) => report_failure_once(
                        &data_failure_tx,
                        &data_failure_reported,
                        MicrophoneCaptureFailureKind::DownstreamDisconnected,
                        "Recorder callback buffer pool disconnected; recording stopped",
                    ),
                    Err(error) => report_failure_once(
                        &data_failure_tx,
                        &data_failure_reported,
                        MicrophoneCaptureFailureKind::UnsupportedFormat,
                        &format!("microphone input could not be converted: {error}"),
                    ),
                }
            },
            move |error| {
                report_failure_once(
                    &failure_tx,
                    &failure_reported,
                    MicrophoneCaptureFailureKind::InputInterrupted,
                    &format!("microphone input was interrupted: {error}"),
                );
            },
            None,
        )
    }

    fn report_failure_once(
        tx: &SyncSender<MicrophoneCaptureFailure>,
        reported: &AtomicBool,
        kind: MicrophoneCaptureFailureKind,
        message: &str,
    ) {
        if reported.swap(true, Ordering::AcqRel) {
            return;
        }
        let failure = MicrophoneCaptureFailure {
            kind,
            message: message.to_string(),
        };
        match tx.try_send(failure) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                log::warn!("microphone failure queue is full; keeping the first failure");
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use platform::{
    microphone_delivery_failure, MicrophoneCaptureFailure, MicrophoneCaptureFailureKind,
    MicrophoneCaptureInfo, MicrophoneCaptureSession, MicrophoneCaptureStart,
};
