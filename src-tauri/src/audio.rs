//! Audio ingestion: normalize any audio or video file to 16 kHz mono f32 PCM.
//!
//! ffmpeg extracts audio from video and resamples audio identically, so both
//! input kinds go through one path — no need to branch on file type.

use crate::error::{AppError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Whisper's required input rate.
pub const SAMPLE_RATE: u32 = 16_000;

/// Run ffmpeg to produce a temporary 16 kHz mono WAV from `input`.
/// The caller owns the returned file and should delete it when done.
pub fn normalize_to_wav(input: &Path) -> Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let out = std::env::temp_dir().join(format!("whisperpilot-{nanos}.wav"));

    let output = Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(input)
        .args([
            "-vn", // drop any video stream
            "-ac", "1", // mono
            "-ar", "16000", // 16 kHz
            "-f", "wav",
        ])
        .arg(&out)
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

    Ok(out)
}

/// Decode a 16 kHz mono 16-bit WAV into normalized f32 samples in [-1, 1].
pub fn decode_wav_16k_mono(path: &Path) -> Result<Vec<f32>> {
    let reader = hound::WavReader::open(path).map_err(|e| AppError::Audio(e.to_string()))?;
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

/// Convenience: normalize `input` through ffmpeg, decode it, and clean up the
/// temporary WAV.
pub fn load_samples(input: &Path) -> Result<Vec<f32>> {
    let wav = normalize_to_wav(input)?;
    let result = decode_wav_16k_mono(&wav);
    let _ = std::fs::remove_file(&wav);
    result
}
