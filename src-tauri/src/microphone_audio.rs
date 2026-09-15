//! Microphone audio preparation for Recorder capture.
//!
//! Native input callbacks are downmixed to mono without changing the hardware
//! sample rate. Provider-specific consumers can then derive a band-limited
//! stream at the exact rate their ASR contract requires.

mod normalizer;

use std::fmt;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError};

mod resampler;

pub use resampler::BandlimitedChunkResampler;

pub use normalizer::{
    IntoMicrophoneF32, MicrophoneAudioError, MicrophoneChunkNormalizer, MicrophoneClockSpan,
    ReusableMicrophoneChunkBuffer,
};
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

    /// A checkout rejected before it is handed to the consumer must still be
    /// returned. Keeping malformed test/device storage in the pool makes the
    /// invariant failure deterministic instead of turning the next callback
    /// into a misleading pool-exhaustion failure.
    fn recycle_rejected(&self, mut samples: Vec<f32>) {
        samples.clear();
        let _ = self.tx.try_send(samples);
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

#[cfg(test)]
mod tests {
    use super::{MicrophoneBufferPoolError, ReusableMicrophoneBufferPool};

    #[test]
    fn invalid_storage_is_returned_to_the_callback_pool() {
        let mut pool =
            ReusableMicrophoneBufferPool::from_storage(1, 2, vec![Vec::with_capacity(1)])
                .expect("a deliberately undersized test buffer is accepted at construction");

        let first = pool.prepare_from(&[0.1_f32]);
        assert!(matches!(
            first,
            Err(MicrophoneBufferPoolError::InvalidStorage)
        ));

        let second = pool.prepare_from(&[0.2_f32]);
        assert!(
            matches!(second, Err(MicrophoneBufferPoolError::InvalidStorage)),
            "a rejected checkout must be returned instead of draining the pool into Exhausted"
        );
    }
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
            // A malformed pool entry is still owned by the pool. Return it
            // before reporting the invariant violation; otherwise one bad
            // checkout permanently drains the bounded callback pool and turns
            // the next callback into a misleading `Exhausted` failure.
            self.recycler.recycle_rejected(samples);
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
