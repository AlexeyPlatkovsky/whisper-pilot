//! Audio ingestion: normalize any audio or video file to 16 kHz mono f32 PCM.
//!
//! ffmpeg extracts audio from video and resamples audio identically, so both
//! input kinds go through one path — no need to branch on file type.
//! ffmpeg writes WAV to stdout (pipe:1); hound decodes from memory —
//! no temp file on disk.

use crate::error::{AppError, Result};
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

/// Whisper's required input rate.
pub const SAMPLE_RATE: u32 = 16_000;

/// Run ffmpeg to produce 16 kHz mono WAV bytes in memory from `input`.
/// Writes to stdout (pipe:1) — no temp file.
pub fn normalize_to_memory(input: &Path) -> Result<Vec<u8>> {
    let output = Command::new("ffmpeg")
        .args(["-i"])
        .arg(input)
        .args([
            "-vn", // drop any video stream
            "-ac", "1", // mono
            "-ar", "16000", // 16 kHz
            "-f", "wav",
            "pipe:1",
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
/// No temp file — everything stays in memory.
pub fn load_samples(input: &Path) -> Result<Vec<f32>> {
    let wav_bytes = normalize_to_memory(input)?;
    decode_wav_16k_mono(&wav_bytes)
}
