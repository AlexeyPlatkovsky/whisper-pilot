use std::f32::consts::TAU;
use whisperpilot_lib::microphone_audio::{
    BandlimitedChunkResampler, MicrophoneChunkNormalizer, PooledMicrophoneSamples,
    ReusableMicrophoneBufferPool, ReusableMicrophoneChunkBuffer,
};

const INFO_PLIST: &str = include_str!("../Info.plist");

fn assert_samples_close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() <= 1e-6,
            "sample {index} differs: actual={actual}, expected={expected}"
        );
    }
}

fn sine(sample_rate: u32, frequency: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|frame| (TAU * frequency * frame as f32 / sample_rate as f32).sin())
        .collect()
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
}

fn resample_all(source_rate: u32, target_rate: u32, input: &[f32]) -> Vec<f32> {
    let mut resampler = BandlimitedChunkResampler::new(source_rate, target_rate).unwrap();
    let mut output = resampler.push_f32(input).unwrap().samples;
    output.extend(resampler.finish().unwrap().samples);
    output
}

fn pooled_allocation(chunk: &PooledMicrophoneSamples) -> (*const f32, usize) {
    (chunk.samples().as_ptr(), chunk.capacity())
}

#[test]
fn downmixes_stereo_while_preserving_native_rate_and_every_frame() {
    let mut normalizer = MicrophoneChunkNormalizer::new(48_000, 2).unwrap();

    let chunk = normalizer
        .push_f32(&[1.0, -1.0, 0.75, 0.25, -0.5, -0.25])
        .unwrap();

    assert_eq!(chunk.start_sample, 0);
    assert_eq!(chunk.captured_end_sample, 3);
    assert_samples_close(&chunk.samples, &[0.0, 0.5, -0.375]);
}

#[test]
fn native_clock_is_monotonic_across_callback_boundaries() {
    let mut normalizer = MicrophoneChunkNormalizer::new(48_000, 1).unwrap();
    let first = normalizer.push_f32(&[0.0, 0.1, 0.2]).unwrap();
    let second = normalizer.push_f32(&[0.3, 0.4]).unwrap();

    assert_eq!(first.start_sample, 0);
    assert_eq!(first.captured_end_sample, 3);
    assert_samples_close(&first.samples, &[0.0, 0.1, 0.2]);
    assert_eq!(second.start_sample, 3);
    assert_eq!(second.captured_end_sample, 5);
    assert_samples_close(&second.samples, &[0.3, 0.4]);
}

#[test]
fn native_downmix_preserves_silence_without_upsampling_or_downsampling() {
    let mut normalizer = MicrophoneChunkNormalizer::new(44_100, 2).unwrap();

    let chunk = normalizer.push_f32(&vec![0.0; 4_410 * 2]).unwrap();

    assert_eq!(chunk.samples.len(), 4_410);
    assert_eq!(chunk.captured_end_sample, 4_410);
    assert!(chunk.samples.iter().all(|sample| *sample == 0.0));
}

#[test]
fn rejects_invalid_hardware_formats_and_partial_frames() {
    assert!(MicrophoneChunkNormalizer::new(0, 1).is_err());
    assert!(MicrophoneChunkNormalizer::new(48_000, 0).is_err());

    let mut normalizer = MicrophoneChunkNormalizer::new(48_000, 2).unwrap();
    assert!(normalizer.push_f32(&[0.1, 0.2, 0.3]).is_err());
}

#[test]
fn reusable_callback_buffer_keeps_allocation_and_clock_for_i16_and_f32_input() {
    let mut buffer = ReusableMicrophoneChunkBuffer::new(2, 4).unwrap();
    let warmup = buffer
        .prepare_from(&[0.0_f32, 0.0, 0.25, 0.75, -0.5, 0.5, 1.0, -1.0])
        .unwrap();
    assert_eq!(warmup.start_sample, 0);
    assert_eq!(warmup.captured_end_sample, 4);
    assert_samples_close(buffer.samples(), &[0.0, 0.5, 0.0, 0.0]);

    let warmed_capacity = buffer.capacity();
    let warmed_pointer = buffer.samples().as_ptr();

    let integer = buffer
        .prepare_from(&[16_384_i16, -16_384, 8_192, 24_576])
        .unwrap();
    assert_eq!(integer.start_sample, 4);
    assert_eq!(integer.captured_end_sample, 6);
    assert_eq!(buffer.capacity(), warmed_capacity);
    assert_eq!(buffer.samples().as_ptr(), warmed_pointer);
    assert_samples_close(buffer.samples(), &[0.0, 0.5]);

    let float = buffer
        .prepare_from(&[0.5_f32, -0.5, 0.25, 0.75, -1.0, 0.0])
        .unwrap();
    assert_eq!(float.start_sample, 6);
    assert_eq!(float.captured_end_sample, 9);
    assert_eq!(buffer.capacity(), warmed_capacity);
    assert_eq!(buffer.samples().as_ptr(), warmed_pointer);
    assert_samples_close(buffer.samples(), &[0.0, 0.5, -0.5]);
}

#[test]
fn pooled_chunk_drop_returns_the_same_allocation_for_the_next_callback() {
    let mut pool = ReusableMicrophoneBufferPool::new(2, 4, 1).unwrap();
    let first = pool.prepare_from(&[0.0_f32, 1.0, -1.0, 1.0]).unwrap();
    assert_eq!(first.start_sample(), 0);
    assert_eq!(first.captured_end_sample(), 2);
    assert_samples_close(first.samples(), &[0.5, 0.0]);
    let first_allocation = pooled_allocation(&first);

    drop(first);

    let second = pool
        .prepare_from(&[0_i16, 16_384, -16_384, 16_384])
        .unwrap();
    assert_eq!(second.start_sample(), 2);
    assert_eq!(second.captured_end_sample(), 4);
    assert_samples_close(second.samples(), &[0.25, 0.0]);
    assert_eq!(pooled_allocation(&second), first_allocation);
}

#[test]
fn pool_rejects_undersized_storage_and_recovers_from_malformed_input() {
    let undersized = Vec::<f32>::with_capacity(3);
    let mut undersized_pool =
        ReusableMicrophoneBufferPool::from_storage(2, 4, vec![undersized]).unwrap();
    assert!(undersized_pool.prepare_from(&[0.0_f32, 0.0]).is_err());

    let valid_storage = Vec::<f32>::with_capacity(4);
    let valid_pointer = valid_storage.as_ptr();
    let mut pool = ReusableMicrophoneBufferPool::from_storage(2, 4, vec![valid_storage]).unwrap();
    assert!(pool.prepare_from(&[0.0_f32, 0.0, 0.5]).is_err());

    let recovered = pool.prepare_from(&[0.0_f32, 1.0, -1.0, 1.0]).unwrap();
    assert_eq!(recovered.samples().as_ptr(), valid_pointer);
    assert_samples_close(recovered.samples(), &[0.5, 0.0]);
}

#[test]
fn bandlimited_resampler_finishes_exact_target_lengths_with_monotonic_clock() {
    let input = sine(48_000, 1_000.0, 4_800);

    for (target_rate, expected_len) in [(16_000, 1_600), (24_000, 2_400)] {
        let mut resampler = BandlimitedChunkResampler::new(48_000, target_rate).unwrap();
        let chunks = [
            resampler.push_f32(&input[..1_777]).unwrap(),
            resampler.push_f32(&input[1_777..]).unwrap(),
            resampler.finish().unwrap(),
        ];

        let mut next_start = 0_u64;
        for chunk in &chunks {
            assert_eq!(chunk.start_sample, next_start);
            assert_eq!(
                chunk.captured_end_sample,
                chunk.start_sample + chunk.samples.len() as u64
            );
            next_start = chunk.captured_end_sample;
        }
        assert_eq!(next_start, expected_len);
    }
}

#[test]
fn bandlimited_resampling_is_equivalent_for_split_and_one_shot_input() {
    let input: Vec<f32> = sine(48_000, 1_000.0, 4_800)
        .into_iter()
        .zip(sine(48_000, 6_000.0, 4_800))
        .map(|(low, high)| low * 0.75 + high * 0.25)
        .collect();
    let expected = resample_all(48_000, 16_000, &input);

    let mut split = BandlimitedChunkResampler::new(48_000, 16_000).unwrap();
    let mut actual = Vec::new();
    for callback in [
        &input[..701],
        &input[701..2_309],
        &input[2_309..4_111],
        &input[4_111..],
    ] {
        actual.extend(split.push_f32(callback).unwrap().samples);
    }
    actual.extend(split.finish().unwrap().samples);

    assert_samples_close(&actual, &expected);
}

#[test]
fn same_rate_bandlimited_resampler_is_an_exact_passthrough() {
    let input = vec![-1.0, -0.25, 0.0, 0.5, 1.0];
    let mut resampler = BandlimitedChunkResampler::new(16_000, 16_000).unwrap();

    let chunk = resampler.push_f32(&input).unwrap();
    let tail = resampler.finish().unwrap();

    assert_eq!(chunk.start_sample, 0);
    assert_eq!(chunk.captured_end_sample, input.len() as u64);
    assert_eq!(chunk.samples, input);
    assert_eq!(tail.start_sample, input.len() as u64);
    assert_eq!(tail.captured_end_sample, input.len() as u64);
    assert!(tail.samples.is_empty());
}

#[test]
fn downsampling_strongly_attenuates_frequencies_above_target_nyquist() {
    let in_band = resample_all(48_000, 16_000, &sine(48_000, 1_000.0, 48_000));
    let above_nyquist = resample_all(48_000, 16_000, &sine(48_000, 12_000.0, 48_000));

    let in_band_rms = rms(&in_band);
    let rejected_rms = rms(&above_nyquist);
    assert!(
        in_band_rms > 0.5,
        "in-band RMS was unexpectedly low: {in_band_rms}"
    );
    assert!(
        rejected_rms < in_band_rms * 0.2,
        "12 kHz should be rejected before 16 kHz decimation: in-band RMS={in_band_rms}, rejected RMS={rejected_rms}"
    );
}

#[test]
fn bandlimited_resampler_rejects_zero_sample_rates() {
    assert!(BandlimitedChunkResampler::new(0, 16_000).is_err());
    assert!(BandlimitedChunkResampler::new(48_000, 0).is_err());
}

#[test]
fn bundle_declares_recorder_specific_microphone_usage() {
    assert!(INFO_PLIST.contains("<key>NSMicrophoneUsageDescription</key>"));
    assert!(INFO_PLIST.contains(
        "WhisperPilot records microphone audio for local Recorder transcription and playback."
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn disconnected_consumer_is_an_actionable_capture_failure() {
    use whisperpilot_lib::microphone_audio::{
        microphone_delivery_failure, MicrophoneCaptureFailureKind,
    };
    use whisperpilot_lib::streaming_audio::QueueSendOutcome;

    let failure = microphone_delivery_failure(QueueSendOutcome::Disconnected)
        .expect("a disconnected Recorder consumer must stop capture");

    assert_eq!(
        failure.kind,
        MicrophoneCaptureFailureKind::DownstreamDisconnected
    );
    assert!(failure.message.contains("consumer"));
    assert!(microphone_delivery_failure(QueueSendOutcome::Sent).is_none());
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires explicit microphone consent and audible input"]
fn real_default_microphone_emits_native_rate_non_silent_audio() {
    use std::sync::mpsc::sync_channel;
    use std::time::{Duration, Instant};
    use whisperpilot_lib::microphone_audio::MicrophoneCaptureSession;

    let (samples_tx, samples_rx) = sync_channel(32);
    let capture = MicrophoneCaptureSession::start(samples_tx)
        .expect("the default microphone should start after explicit consent");
    assert_eq!(capture.info.sample_rate, capture.info.source_sample_rate);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut observed_non_silent = false;
    while Instant::now() < deadline {
        let Ok(chunk) = samples_rx.recv_timeout(Duration::from_millis(250)) else {
            continue;
        };
        observed_non_silent |= chunk.samples().iter().any(|sample| sample.abs() > 0.000_1);
        if observed_non_silent {
            break;
        }
    }
    drop(capture.session);

    assert!(
        observed_non_silent,
        "the microphone delivered only silence; verify macOS permission and speak during the test"
    );
}
