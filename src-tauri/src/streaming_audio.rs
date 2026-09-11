//! Streaming audio capture: macOS system audio only, via ScreenCaptureKit.
//! Local transcription receives native 16 kHz mono f32 samples; cloud
//! providers may request another ScreenCaptureKit-supported rate, currently
//! 24 kHz for OpenAI. Streaming never opens or mixes the microphone.

use crate::error::{AppError, Result};
#[cfg(target_os = "macos")]
use std::collections::VecDeque;
#[cfg(target_os = "macos")]
use std::sync::mpsc::{SyncSender, TrySendError};
#[cfg(target_os = "macos")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "macos")]
use std::time::Duration;

/// How often the capture pump drains ScreenCaptureKit samples into the
/// session. This is independent of the transcription engine's decode window.
#[cfg(target_os = "macos")]
const CAPTURE_TICK: Duration = Duration::from_millis(100);

/// Four seconds at the highest native rate currently supported by Streaming.
/// The ScreenCaptureKit callback never grows this staging buffer past the
/// limit even if the pump is delayed.
#[cfg(target_os = "macos")]
pub const NATIVE_SAMPLE_BUFFER_CAPACITY: usize = 24_000 * 4;

/// One delivered native span plus its position on the original capture
/// clock. `captured_end_sample` can be greater than `start_sample + len`
/// when the native staging buffer dropped its newest tail.
#[derive(Debug, Clone, PartialEq)]
pub struct CapturedAudioChunk {
    pub start_sample: u64,
    pub captured_end_sample: u64,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueSendOutcome {
    Sent,
    DroppedNewest,
    Disconnected,
}

#[cfg(target_os = "macos")]
pub fn try_send_drop_newest(
    tx: &SyncSender<CapturedAudioChunk>,
    chunk: CapturedAudioChunk,
) -> QueueSendOutcome {
    match tx.try_send(chunk) {
        Ok(()) => QueueSendOutcome::Sent,
        Err(TrySendError::Full(_)) => QueueSendOutcome::DroppedNewest,
        Err(TrySendError::Disconnected(_)) => QueueSendOutcome::Disconnected,
    }
}

/// Immutable capture requirements selected before a stream starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamingCaptureSpec {
    pub sample_rate: u32,
    pub microphone: bool,
    pub system_audio: bool,
}

/// Builds the only capture shape Streaming supports. Keeping the supported
/// rates explicit prevents a provider configuration from silently requesting
/// a rate that ScreenCaptureKit cannot provide natively.
pub fn system_audio_capture_spec(sample_rate: u32) -> Result<StreamingCaptureSpec> {
    if !matches!(sample_rate, 16_000 | 24_000) {
        return Err(AppError::Capture(format!(
            "unsupported Streaming system-audio sample rate: {sample_rate} Hz"
        )));
    }

    Ok(StreamingCaptureSpec {
        sample_rate,
        microphone: false,
        system_audio: true,
    })
}

#[cfg(target_os = "macos")]
struct NativeSampleBuffer {
    samples: VecDeque<f32>,
    start_sample: u64,
    next_sample: u64,
}

#[cfg(target_os = "macos")]
type SharedBuffer = Arc<Mutex<NativeSampleBuffer>>;

#[cfg(target_os = "macos")]
fn new_shared_buffer() -> SharedBuffer {
    Arc::new(Mutex::new(NativeSampleBuffer {
        samples: VecDeque::new(),
        start_sample: 0,
        next_sample: 0,
    }))
}

#[cfg(target_os = "macos")]
fn drain(buf: &SharedBuffer) -> CapturedAudioChunk {
    let mut guard = buf.lock().expect("streaming audio buffer mutex poisoned");
    let start_sample = guard.start_sample;
    let captured_end_sample = guard.next_sample;
    let samples = guard.samples.drain(..).collect();
    guard.start_sample = captured_end_sample;
    CapturedAudioChunk {
        start_sample,
        captured_end_sample,
        samples,
    }
}

#[cfg(target_os = "macos")]
/// Returns `true` when newest samples did not fit and were dropped.
fn push(buf: &SharedBuffer, samples: &[f32]) -> bool {
    let mut guard = buf.lock().expect("streaming audio buffer mutex poisoned");
    let callback_start = guard.next_sample;
    guard.next_sample = guard.next_sample.saturating_add(samples.len() as u64);
    if guard.samples.is_empty() {
        guard.start_sample = callback_start;
    }
    let available = NATIVE_SAMPLE_BUFFER_CAPACITY.saturating_sub(guard.samples.len());
    guard
        .samples
        .extend(samples.iter().take(available).copied());
    samples.len() > available
}

/// Drains native system-audio samples without mixing or resampling. Empty
/// chunks are retained as liveness signals for the downstream windowing loop.
#[cfg(target_os = "macos")]
fn run_capture_pump(
    system_buf: SharedBuffer,
    tx: SyncSender<CapturedAudioChunk>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    loop {
        std::thread::sleep(CAPTURE_TICK);
        let stopping = stop.load(Ordering::Acquire);
        let chunk = drain(&system_buf);
        if stopping {
            if tx.send(chunk).is_err() {
                log::warn!("Streaming audio final drain had no receiver");
            }
            return;
        }
        match try_send_drop_newest(&tx, chunk) {
            QueueSendOutcome::Sent => {}
            QueueSendOutcome::DroppedNewest => {
                log::warn!("Streaming audio ingress overloaded; dropping newest capture chunk");
            }
            QueueSendOutcome::Disconnected => return,
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use screencapturekit::prelude::*;

    /// Owns a running `SCStream` capturing system audio only (no video).
    struct SystemAudioCapture {
        stream: SCStream,
    }

    struct SystemAudioHandler {
        buf: SharedBuffer,
    }

    impl SCStreamOutputTrait for SystemAudioHandler {
        fn did_output_sample_buffer(
            &self,
            sample: screencapturekit::cm::CMSampleBuffer,
            of_type: SCStreamOutputType,
        ) {
            if !matches!(of_type, SCStreamOutputType::Audio) {
                return;
            }
            use screencapturekit::cm::CMSampleBufferExt;
            let Some(list) = sample.audio_buffer_list() else {
                return;
            };
            let Some(buffer) = list.get(0) else {
                return;
            };
            let bytes = buffer.data();
            let samples: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();
            if push(&self.buf, &samples) {
                log::warn!(
                    "Streaming native audio buffer overloaded; dropping newest capture samples"
                );
            }
        }
    }

    impl SystemAudioCapture {
        fn start(buf: SharedBuffer, sample_rate: u32) -> Result<Self> {
            use screencapturekit::stream::configuration::audio::{
                AudioChannelCount, AudioSampleRate,
            };

            let native_rate = AudioSampleRate::from_hz(sample_rate as i32).ok_or_else(|| {
                AppError::Capture(format!(
                    "unsupported ScreenCaptureKit sample rate: {sample_rate} Hz"
                ))
            })?;
            let content = SCShareableContent::get()
                .map_err(|error| AppError::Capture(format!("no shareable content: {error}")))?;
            let display = content.displays().into_iter().next().ok_or_else(|| {
                AppError::Capture("no display available for system-audio capture".into())
            })?;
            let filter = SCContentFilter::create()
                .with_display(&display)
                .with_excluding_windows(&[])
                .build();
            let config = SCStreamConfiguration::new()
                .with_captures_audio(true)
                .with_sample_rate(native_rate)
                .with_channel_count(AudioChannelCount::Mono);

            let mut stream = SCStream::new(&filter, &config);
            stream.add_output_handler(SystemAudioHandler { buf }, SCStreamOutputType::Audio);
            stream.start_capture().map_err(|error| {
                AppError::Capture(format!("failed to start system audio: {error}"))
            })?;

            Ok(Self { stream })
        }
    }

    impl Drop for SystemAudioCapture {
        fn drop(&mut self) {
            if let Err(error) = self.stream.stop_capture() {
                log::warn!("failed to stop system-audio capture cleanly: {error}");
            }
        }
    }

    /// A running system-audio capture session at its engine-selected rate.
    pub struct StreamingSession {
        system: Option<SystemAudioCapture>,
        stop: Arc<std::sync::atomic::AtomicBool>,
        capture_thread: Option<std::thread::JoinHandle<()>>,
    }

    impl StreamingSession {
        pub fn start(
            tx: SyncSender<CapturedAudioChunk>,
            spec: StreamingCaptureSpec,
        ) -> Result<Self> {
            if spec.microphone || !spec.system_audio {
                return Err(AppError::Capture(
                    "Streaming capture must use system audio without a microphone".into(),
                ));
            }
            system_audio_capture_spec(spec.sample_rate)?;

            let system_buf = new_shared_buffer();
            let system = SystemAudioCapture::start(system_buf.clone(), spec.sample_rate)?;
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let capture_thread = {
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || run_capture_pump(system_buf, tx, stop))
            };

            Ok(Self {
                system: Some(system),
                stop,
                capture_thread: Some(capture_thread),
            })
        }
    }

    impl Drop for StreamingSession {
        fn drop(&mut self) {
            // Stop the producer before asking the pump for its final drain so
            // no callback can append after that last buffer snapshot.
            self.system.take();
            self.stop.store(true, std::sync::atomic::Ordering::Release);
            if let Some(handle) = self.capture_thread.take() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use platform::StreamingSession;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;
    use std::time::{Duration, Instant};

    #[test]
    fn capture_spec_supports_only_native_product_rates() {
        assert_eq!(
            system_audio_capture_spec(16_000).unwrap().sample_rate,
            16_000
        );
        assert_eq!(
            system_audio_capture_spec(24_000).unwrap().sample_rate,
            24_000
        );
        assert!(system_audio_capture_spec(44_100).is_err());
    }

    // WP-114 S-2: the callback-side queue must never wait for a slow
    // consumer. Once full, the newest chunk is rejected and the caller gets
    // an explicit overload outcome.
    #[test]
    fn bounded_queue_drops_newest_and_reports_overflow_without_blocking() {
        let (tx, rx) = sync_channel(1);
        assert_eq!(
            try_send_drop_newest(
                &tx,
                CapturedAudioChunk {
                    start_sample: 0,
                    captured_end_sample: 1,
                    samples: vec![1.0_f32],
                },
            ),
            QueueSendOutcome::Sent
        );

        let started = Instant::now();
        let outcome = try_send_drop_newest(
            &tx,
            CapturedAudioChunk {
                start_sample: 1,
                captured_end_sample: 2,
                samples: vec![2.0_f32],
            },
        );

        assert_eq!(outcome, QueueSendOutcome::DroppedNewest);
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "a full live-audio queue must not block the producer"
        );
        assert_eq!(
            rx.recv().expect("first chunk remains queued").samples,
            vec![1.0]
        );
        assert!(rx.try_recv().is_err(), "the newest chunk must be dropped");
    }

    // WP-114 sustained-overload boundary: the native callback buffer itself
    // is bounded too, before any channel send can occur.
    #[test]
    fn native_sample_buffer_never_exceeds_its_declared_capacity() {
        let buffer = new_shared_buffer();
        let dropped = push(
            &buffer,
            &vec![0.25_f32; NATIVE_SAMPLE_BUFFER_CAPACITY + 1_024],
        );

        assert!(
            dropped,
            "overflow must be observable at the producer boundary"
        );
        assert!(
            buffer.lock().expect("sample-buffer mutex").samples.len()
                <= NATIVE_SAMPLE_BUFFER_CAPACITY,
            "native samples must remain bounded when the capture pump is slow"
        );
    }

    #[test]
    fn stopping_pump_forwards_the_final_staged_samples_before_disconnect() {
        let buffer = new_shared_buffer();
        assert!(!push(&buffer, &[0.1, 0.2, 0.3]));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (tx, rx) = sync_channel(1);

        run_capture_pump(buffer, tx, stop);

        let final_chunk = rx.recv().expect("final staged chunk");
        assert_eq!(final_chunk.start_sample, 0);
        assert_eq!(final_chunk.captured_end_sample, 3);
        assert_eq!(final_chunk.samples, vec![0.1, 0.2, 0.3]);
        assert!(
            rx.recv().is_err(),
            "the sender closes after the final drain"
        );
    }
}
