use super::MicrophoneAudioError;
use crate::streaming_audio::CapturedAudioChunk;
use rubato::audioadapter::{Adapter, AdapterMut};
use rubato::{
    calculate_cutoff, Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};
use std::collections::VecDeque;

const RESAMPLER_INPUT_CHUNK_FRAMES: usize = 1_024;
const RESAMPLER_SINC_LENGTH: usize = 128;

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
