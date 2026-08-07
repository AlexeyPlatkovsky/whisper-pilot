//! Streaming audio capture: microphone + macOS system-audio (loopback),
//! mixed into one continuous 16 kHz mono f32 stream for a Streaming session.
//!
//! WP-70 owns capture and mixing only. Windowing the continuous stream into
//! ~5-10s decode chunks, and everything downstream of that, is WP-71's
//! concern — this module hands its consumer a plain, unbounded stream of
//! samples at Whisper's required rate/format (`crate::audio::SAMPLE_RATE`).
//!
//! Mic-only degradation (WP-68 D-Capture-fallback): if system-audio capture
//! is unavailable or its permission is denied, a session still starts with
//! whichever source(s) actually came up; only "both sources failed" is a
//! hard error. This mirrors this app's other fail-open engine paths
//! (diarization, ADR-013) rather than treating a partial capture failure as
//! fatal.
//!
//! System-audio loopback uses the `screencapturekit` crate. An earlier
//! attempt at this module used the lower-level `objc2-screen-capture-kit`
//! binding specifically to avoid `screencapturekit`'s mandatory `apple-metal`
//! dependency, which needs a full Xcode.app (not just Command Line Tools) to
//! link its Swift compatibility libraries — that constraint no longer
//! applies once Xcode.app is installed, and the ergonomic crate is
//! materially lower-risk than hand-written CoreMedia buffer extraction for a
//! feature whose stated top priority is quality/precision.

use crate::audio::SAMPLE_RATE;
use crate::error::{AppError, Result};
use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often the mixer thread drains the source buffers and emits a mixed
/// chunk to the session's output. Independent of WP-71's own 5-10s decode
/// window — this is just the mixing granularity, kept short so a consumer
/// seeing a continuous stream never waits long for the next chunk.
const MIX_TICK: Duration = Duration::from_millis(100);

/// Which of the two capture sources are actually active in a session —
/// surfaced to the UI (WP-73) so a mic-only degradation is visible rather
/// than silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveSources {
    Both,
    MicOnly,
    SystemAudioOnly,
}

/// Downmix interleaved multi-channel samples to mono by averaging each
/// frame's channels. `channels` must be >= 1; a `channels` of 1 returns the
/// input unchanged (no-op downmix). A trailing partial frame (input length
/// not a multiple of `channels`) is dropped rather than averaged as if it
/// were complete — `chunks_exact`, not `chunks`.
pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

/// Linear-interpolation resample of a mono f32 stream from `input_rate` to
/// `output_rate`. Not broadcast-quality (no anti-aliasing filter), but
/// adequate for speech at the rates involved here — the same trade-off this
/// project already accepts elsewhere for simplicity over audiophile fidelity.
/// A no-op (returns the input unchanged) when the rates already match.
pub fn resample_linear(input: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    if input.is_empty() || input_rate == output_rate {
        return input.to_vec();
    }
    let ratio = output_rate as f64 / input_rate as f64;
    let out_len = ((input.len() as f64) * ratio).round().max(0.0) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let a = input.get(idx).copied().unwrap_or(0.0);
        let b = input.get(idx + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}

/// Mix two mono f32 buffers by summing and clamping to `[-1.0, 1.0]`
/// (standard additive mixing, not averaging — averaging would halve each
/// source's perceived loudness whenever both are present). The shorter
/// buffer is treated as silence for the remainder of the longer one, so
/// output length is always `max(a.len(), b.len())`.
pub fn mix_mono(a: &[f32], b: &[f32]) -> Vec<f32> {
    let len = a.len().max(b.len());
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let sa = a.get(i).copied().unwrap_or(0.0);
        let sb = b.get(i).copied().unwrap_or(0.0);
        out.push((sa + sb).clamp(-1.0, 1.0));
    }
    out
}

/// A thread-safe append-only sample queue one capture callback pushes into
/// and the mixer thread drains from. Plain `Mutex<VecDeque<f32>>` rather than
/// a lock-free ring buffer: capture callbacks here push at most a few
/// thousand samples at a time (tens of ms of audio), so a brief lock is not
/// the bottleneck a real-time audio *output* path would need to avoid.
type SharedBuffer = Arc<Mutex<VecDeque<f32>>>;

fn new_shared_buffer() -> SharedBuffer {
    Arc::new(Mutex::new(VecDeque::new()))
}

/// Drain everything currently queued, in order, and clear the queue.
fn drain(buf: &SharedBuffer) -> Vec<f32> {
    let mut guard = buf.lock().expect("streaming audio buffer mutex poisoned");
    guard.drain(..).collect()
}

fn push(buf: &SharedBuffer, samples: &[f32]) {
    let mut guard = buf.lock().expect("streaming audio buffer mutex poisoned");
    guard.extend(samples.iter().copied());
}

/// Runs on its own thread: every `MIX_TICK`, drains whichever source
/// buffer(s) are present, mixes them (or passes one through unmixed), and
/// sends the result — even an empty chunk, so a consumer can tell the
/// session is still alive versus having silently stopped. Stops when `tx`'s
/// receiver is dropped (send failure) or `stop` is set.
fn run_mixer(
    mic_buf: Option<SharedBuffer>,
    system_buf: Option<SharedBuffer>,
    tx: Sender<Vec<f32>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(MIX_TICK);
        let chunk = match (&mic_buf, &system_buf) {
            (Some(m), Some(s)) => mix_mono(&drain(m), &drain(s)),
            (Some(m), None) => drain(m),
            (None, Some(s)) => drain(s),
            (None, None) => Vec::new(),
        };
        if tx.send(chunk).is_err() {
            return;
        }
    }
}

/// Everything platform-specific (cpal for the microphone, ScreenCaptureKit
/// for system-audio loopback) lives behind this cfg — WhisperPilot is
/// macOS-only (AGENTS.md), and neither dependency is available to a Linux CI
/// build (see the Cargo.toml comment where both are pinned to the
/// macOS-only target section).
#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use screencapturekit::prelude::*;

    /// Owns a running `cpal` input stream; dropping it stops capture.
    struct MicCapture {
        _stream: cpal::Stream,
    }

    impl MicCapture {
        fn start(buf: SharedBuffer) -> Result<Self> {
            use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

            let host = cpal::default_host();
            let device = host
                .default_input_device()
                .ok_or_else(|| AppError::Capture("no default microphone input device".into()))?;
            let supported = device
                .default_input_config()
                .map_err(|e| AppError::Capture(format!("no usable microphone config: {e}")))?;
            let channels = supported.channels();
            let device_rate = supported.sample_rate();
            let config = supported.config();
            let sample_format = supported.sample_format();

            let err_buf = Arc::clone(&buf);
            let error_callback = move |e: cpal::StreamError| {
                log::error!("microphone capture stream error: {e}");
                // Nothing to push on error; the buffer just stops growing and
                // the mixer emits silence for this source until the stream
                // (if it recovers) resumes calling the data callback.
                let _ = &err_buf;
            };

            let stream = match sample_format {
                cpal::SampleFormat::F32 => device
                    .build_input_stream(
                        &config,
                        move |data: &[f32], _| {
                            let mono = downmix_to_mono(data, channels);
                            let resampled = resample_linear(&mono, device_rate, SAMPLE_RATE);
                            push(&buf, &resampled);
                        },
                        error_callback,
                        None,
                    )
                    .map_err(|e| AppError::Capture(format!("failed to open microphone: {e}")))?,
                cpal::SampleFormat::I16 => device
                    .build_input_stream(
                        &config,
                        move |data: &[i16], _| {
                            let as_f32: Vec<f32> =
                                data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                            let mono = downmix_to_mono(&as_f32, channels);
                            let resampled = resample_linear(&mono, device_rate, SAMPLE_RATE);
                            push(&buf, &resampled);
                        },
                        error_callback,
                        None,
                    )
                    .map_err(|e| AppError::Capture(format!("failed to open microphone: {e}")))?,
                other => {
                    return Err(AppError::Capture(format!(
                        "unsupported microphone sample format: {other:?}"
                    )))
                }
            };

            stream
                .play()
                .map_err(|e| AppError::Capture(format!("failed to start microphone: {e}")))?;

            Ok(Self { _stream: stream })
        }
    }

    /// Owns a running `SCStream` capturing system audio only (no video);
    /// dropping it stops capture. Requests 16 kHz mono directly from
    /// ScreenCaptureKit — a natively supported rate/channel-count pair (see
    /// `screencapturekit::stream::configuration::audio`) — so unlike the
    /// microphone path no resampling is needed here.
    struct SystemAudioCapture {
        stream: SCStream,
    }

    /// Receives ScreenCaptureKit's audio callback; holds nothing but the
    /// shared buffer, so it never needs to leave this module.
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
            // Mono capture (requested below) means exactly one buffer,
            // carrying Float32 PCM — the format ScreenCaptureKit's audio
            // path documents for this configuration. Reinterpreting raw
            // bytes as little-endian f32 is the same approach hound takes
            // for WAV samples in audio.rs.
            let Some(buffer) = list.get(0) else {
                return;
            };
            let bytes = buffer.data();
            let samples: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            push(&self.buf, &samples);
        }
    }

    impl SystemAudioCapture {
        fn start(buf: SharedBuffer) -> Result<Self> {
            use screencapturekit::stream::configuration::audio::{
                AudioChannelCount, AudioSampleRate,
            };

            let content = SCShareableContent::get()
                .map_err(|e| AppError::Capture(format!("no shareable content: {e}")))?;
            let display = content.displays().into_iter().next().ok_or_else(|| {
                AppError::Capture("no display available for system-audio capture".into())
            })?;
            let filter = SCContentFilter::create()
                .with_display(&display)
                .with_excluding_windows(&[])
                .build();
            let config = SCStreamConfiguration::new()
                .with_captures_audio(true)
                .with_sample_rate(AudioSampleRate::Rate16000)
                .with_channel_count(AudioChannelCount::Mono);

            let mut stream = SCStream::new(&filter, &config);
            stream.add_output_handler(SystemAudioHandler { buf }, SCStreamOutputType::Audio);
            stream
                .start_capture()
                .map_err(|e| AppError::Capture(format!("failed to start system audio: {e}")))?;

            Ok(Self { stream })
        }
    }

    impl Drop for SystemAudioCapture {
        fn drop(&mut self) {
            if let Err(e) = self.stream.stop_capture() {
                log::warn!("failed to stop system-audio capture cleanly: {e}");
            }
        }
    }

    /// A running Streaming capture session: whichever of mic / system-audio
    /// started successfully, mixed into one continuous stream. At least one
    /// source must start or this returns an error — both failing means
    /// there is genuinely nothing to capture, unlike a single-source
    /// failure, which degrades per this module's fail-open contract.
    pub struct StreamingSession {
        _mic: Option<MicCapture>,
        _system: Option<SystemAudioCapture>,
        active: ActiveSources,
        stop: Arc<std::sync::atomic::AtomicBool>,
        mixer_thread: Option<std::thread::JoinHandle<()>>,
    }

    impl StreamingSession {
        /// `tx` receives mixed 16 kHz mono f32 chunks roughly every
        /// `MIX_TICK`; the caller (WP-71) windows them for decode.
        pub fn start(tx: Sender<Vec<f32>>) -> Result<Self> {
            let mic_buf = new_shared_buffer();
            let system_buf = new_shared_buffer();

            let mic = match MicCapture::start(mic_buf.clone()) {
                Ok(m) => Some(m),
                Err(e) => {
                    log::warn!("microphone capture unavailable, degrading: {e}");
                    None
                }
            };
            let system = match SystemAudioCapture::start(system_buf.clone()) {
                Ok(s) => Some(s),
                Err(e) => {
                    log::warn!("system-audio capture unavailable, degrading: {e}");
                    None
                }
            };

            let active = match (mic.is_some(), system.is_some()) {
                (true, true) => ActiveSources::Both,
                (true, false) => ActiveSources::MicOnly,
                (false, true) => ActiveSources::SystemAudioOnly,
                (false, false) => {
                    return Err(AppError::Capture(
                        "neither microphone nor system-audio capture is available".into(),
                    ))
                }
            };

            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let mixer_thread = {
                let stop = Arc::clone(&stop);
                let mic_buf = mic.as_ref().map(|_| mic_buf);
                let system_buf = system.as_ref().map(|_| system_buf);
                std::thread::spawn(move || run_mixer(mic_buf, system_buf, tx, stop))
            };

            Ok(Self {
                _mic: mic,
                _system: system,
                active,
                stop,
                mixer_thread: Some(mixer_thread),
            })
        }

        pub fn active_sources(&self) -> ActiveSources {
            self.active
        }
    }

    impl Drop for StreamingSession {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(handle) = self.mixer_thread.take() {
                // The mixer thread wakes at most MIX_TICK after `stop` is
                // set; joining bounds session teardown to that, not to the
                // capture streams themselves (already stopped by their own
                // Drop impls by the time this runs, since struct fields drop
                // in declaration order — _mic/_system before this).
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
    fn downmix_mono_is_identity() {
        let input = vec![0.1, -0.2, 0.3];
        assert_eq!(downmix_to_mono(&input, 1), input);
    }

    #[test]
    fn downmix_stereo_averages_channels() {
        // frame 0: L=1.0 R=0.0 -> 0.5; frame 1: L=-1.0 R=1.0 -> 0.0
        let input = vec![1.0, 0.0, -1.0, 1.0];
        assert_eq!(downmix_to_mono(&input, 2), vec![0.5, 0.0]);
    }

    #[test]
    fn downmix_drops_incomplete_trailing_frame() {
        // One full stereo frame plus one leftover sample with no pair.
        let input = vec![1.0, 0.0, 0.5];
        assert_eq!(downmix_to_mono(&input, 2), vec![0.5]);
    }

    #[test]
    fn resample_noop_when_rates_match() {
        let input = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&input, 16_000, 16_000), input);
    }

    #[test]
    fn resample_empty_input_stays_empty() {
        assert!(resample_linear(&[], 44_100, 16_000).is_empty());
    }

    #[test]
    fn resample_downsamples_to_expected_length() {
        // 100 samples at 48kHz -> ~33 samples at 16kHz (exact ratio 1/3).
        let input: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let out = resample_linear(&input, 48_000, 16_000);
        assert_eq!(out.len(), 33);
        // First sample must be preserved exactly (no interpolation at t=0).
        assert_eq!(out[0], 0.0);
    }

    #[test]
    fn resample_upsamples_to_expected_length() {
        // 10 samples at 16kHz -> 20 samples at 32kHz.
        let input: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let out = resample_linear(&input, 16_000, 32_000);
        assert_eq!(out.len(), 20);
    }

    #[test]
    fn resample_interpolates_between_samples() {
        // 2 samples at 1Hz -> 4 samples at 2Hz: midpoint should interpolate.
        let input = vec![0.0, 10.0];
        let out = resample_linear(&input, 1, 2);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0], 0.0);
        // out[1] sits halfway between input[0]=0.0 and input[1]=10.0.
        assert!((out[1] - 5.0).abs() < 1e-4);
    }

    #[test]
    fn mix_sums_and_clamps_overlap() {
        let a = vec![0.6, 0.6];
        let b = vec![0.6, -0.6];
        let out = mix_mono(&a, &b);
        // 0.6 + 0.6 = 1.2 clamps to 1.0; 0.6 + -0.6 = 0.0 unclamped.
        assert_eq!(out, vec![1.0, 0.0]);
    }

    #[test]
    fn mix_pads_shorter_source_with_silence() {
        let a = vec![0.5, 0.5, 0.5];
        let b = vec![0.2];
        let out = mix_mono(&a, &b);
        assert_eq!(out, vec![0.7, 0.5, 0.5]);
    }

    #[test]
    fn mix_two_empty_buffers_is_empty() {
        assert!(mix_mono(&[], &[]).is_empty());
    }

    #[test]
    fn active_sources_variants_are_distinct() {
        // Guards against an accidental future derive/equality regression on
        // this small enum, which the Streaming UI (WP-73) branches on.
        assert_ne!(ActiveSources::Both, ActiveSources::MicOnly);
        assert_ne!(ActiveSources::MicOnly, ActiveSources::SystemAudioOnly);
        assert_ne!(ActiveSources::Both, ActiveSources::SystemAudioOnly);
    }
}
