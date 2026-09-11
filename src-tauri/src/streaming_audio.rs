//! Streaming audio capture: macOS system audio only, via ScreenCaptureKit.
//! Local transcription receives native 16 kHz mono f32 samples; cloud
//! providers may request another ScreenCaptureKit-supported rate, currently
//! 24 kHz for OpenAI. Streaming never opens or mixes the microphone.

use crate::error::{AppError, Result};
#[cfg(target_os = "macos")]
use std::collections::VecDeque;
#[cfg(target_os = "macos")]
use std::sync::mpsc::Sender;
#[cfg(target_os = "macos")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "macos")]
use std::time::Duration;

/// How often the capture pump drains ScreenCaptureKit samples into the
/// session. This is independent of the transcription engine's decode window.
#[cfg(target_os = "macos")]
const CAPTURE_TICK: Duration = Duration::from_millis(100);

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
type SharedBuffer = Arc<Mutex<VecDeque<f32>>>;

#[cfg(target_os = "macos")]
fn new_shared_buffer() -> SharedBuffer {
    Arc::new(Mutex::new(VecDeque::new()))
}

#[cfg(target_os = "macos")]
fn drain(buf: &SharedBuffer) -> Vec<f32> {
    let mut guard = buf.lock().expect("streaming audio buffer mutex poisoned");
    guard.drain(..).collect()
}

#[cfg(target_os = "macos")]
fn push(buf: &SharedBuffer, samples: &[f32]) {
    let mut guard = buf.lock().expect("streaming audio buffer mutex poisoned");
    guard.extend(samples.iter().copied());
}

/// Drains native system-audio samples without mixing or resampling. Empty
/// chunks are retained as liveness signals for the downstream windowing loop.
#[cfg(target_os = "macos")]
fn run_capture_pump(
    system_buf: SharedBuffer,
    tx: Sender<Vec<f32>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(CAPTURE_TICK);
        if tx.send(drain(&system_buf)).is_err() {
            return;
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
            push(&self.buf, &samples);
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
        _system: SystemAudioCapture,
        stop: Arc<std::sync::atomic::AtomicBool>,
        capture_thread: Option<std::thread::JoinHandle<()>>,
    }

    impl StreamingSession {
        pub fn start(tx: Sender<Vec<f32>>, spec: StreamingCaptureSpec) -> Result<Self> {
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
                _system: system,
                stop,
                capture_thread: Some(capture_thread),
            })
        }
    }

    impl Drop for StreamingSession {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
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
}
