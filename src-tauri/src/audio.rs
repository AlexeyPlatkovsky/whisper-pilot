//! Audio ingestion: normalize any audio or video file to 16 kHz mono f32 PCM.
//!
//! ffmpeg extracts audio from video and resamples audio identically, so both
//! input kinds go through one path — no need to branch on file type.
//! ffmpeg writes raw PCM to stdout (pipe:1); a valid WAV header is prepended
//! in memory so hound can decode without a temp file on disk.

use crate::error::{AppError, Result};
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

/// Whisper's required input rate.
pub const SAMPLE_RATE: u32 = 16_000;

const BITS_PER_SAMPLE: u16 = 16;
const NUM_CHANNELS: u16 = 1;

/// Build a canonical 44-byte WAV header for 16-bit mono PCM at `SAMPLE_RATE` Hz.
fn wav_header(pcm_len: u32) -> [u8; 44] {
    let byte_rate = SAMPLE_RATE * NUM_CHANNELS as u32 * (BITS_PER_SAMPLE / 8) as u32;
    let block_align = NUM_CHANNELS * (BITS_PER_SAMPLE / 8);
    let riff_size = 36 + pcm_len;

    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&riff_size.to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM format
    h[22..24].copy_from_slice(&NUM_CHANNELS.to_le_bytes());
    h[24..28].copy_from_slice(&SAMPLE_RATE.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&block_align.to_le_bytes());
    h[34..36].copy_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&pcm_len.to_le_bytes());
    h
}

/// Run ffmpeg to produce 16 kHz mono raw PCM in memory from `input`.
/// Returns raw s16le bytes — caller prepends a WAV header for hound.
pub fn normalize_to_memory(input: &Path) -> Result<Vec<u8>> {
    let output = Command::new("ffmpeg")
        .args(["-i"])
        .arg(input)
        .args([
            "-vn", // drop any video stream
            "-ac", "1", // mono
            "-ar", "16000", // 16 kHz
            "-f", "s16le", "pipe:1",
        ])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::FfmpegMissing
            } else {
                AppError::Ffmpeg(e.to_string())
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // ffmpeg is verbose; surface only the tail, which carries the reason.
        let tail: String = stderr.lines().rev().take(3).collect::<Vec<_>>().join(" | ");
        return Err(AppError::Ffmpeg(tail));
    }

    Ok(output.stdout)
}

/// Decode 16 kHz mono WAV bytes into normalized f32 samples in [-1, 1].
pub fn decode_wav_16k_mono(data: &[u8]) -> Result<Vec<f32>> {
    let cursor = Cursor::new(data);
    let reader = hound::WavReader::new(cursor).map_err(|e| AppError::Audio(e.to_string()))?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != SAMPLE_RATE {
        return Err(AppError::Audio(format!(
            "expected 16 kHz mono, got {} Hz / {} ch",
            spec.sample_rate, spec.channels
        )));
    }

    let mut reader = reader;
    let samples: std::result::Result<Vec<f32>, _> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect(),
        hound::SampleFormat::Float => reader.samples::<f32>().collect(),
    };
    samples.map_err(|e| AppError::Audio(e.to_string()))
}

/// Convenience: normalize `input` through ffmpeg and decode it.
/// Assembles the WAV header + raw PCM in memory — no temp file.
pub fn load_samples(input: &Path) -> Result<Vec<f32>> {
    let pcm = normalize_to_memory(input)?;
    let header = wav_header(pcm.len() as u32);
    let mut wav = Vec::with_capacity(header.len() + pcm.len());
    wav.extend_from_slice(&header);
    wav.extend_from_slice(&pcm);
    decode_wav_16k_mono(&wav)
}
