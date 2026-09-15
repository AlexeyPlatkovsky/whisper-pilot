use crate::streaming_audio::CapturedAudioChunk;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicrophoneAudioError(pub(crate) String);

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
