//! Audio ingestion: normalize any audio or video file to 16 kHz mono f32 PCM.
//!
//! ffmpeg extracts audio from video and resamples audio identically, so both
//! input kinds go through one path — no need to branch on file type.
//! ffmpeg writes raw PCM to stdout (pipe:1), which is converted directly to
//! normalized samples without constructing a second in-memory WAV copy.

use crate::error::{AppError, Result};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Whisper's required input rate.
pub const SAMPLE_RATE: u32 = 16_000;

/// Resolve the ffmpeg binary, searching PATH first, then common Homebrew
/// locations.  GUI apps on macOS inherit a minimal PATH that doesn't include
/// Homebrew's prefix, so a bare `Command::new("ffmpeg")` fails despite ffmpeg
/// being installed.
fn resolve_ffmpeg() -> PathBuf {
    let name = "ffmpeg";

    // Walk PATH manually (no dependency on the `which` crate).
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }

    // Homebrew prefixes (Apple Silicon → Intel order).
    for candidate in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"] {
        let p = PathBuf::from(candidate);
        if p.is_file() {
            return p;
        }
    }

    // Fall back to bare name so Command::new produces a clear NotFound error.
    PathBuf::from(name)
}

/// Run ffmpeg to produce 16 kHz mono raw PCM in memory from `input`.
/// Returns raw s16le bytes for direct sample conversion.
pub fn normalize_to_memory(input: &Path) -> Result<Vec<u8>> {
    let output = Command::new(resolve_ffmpeg())
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

/// Convert ffmpeg's canonical mono s16le output directly to normalized f32.
/// Reject an incomplete final sample instead of silently dropping it.
pub fn decode_pcm_s16le(data: &[u8]) -> Result<Vec<f32>> {
    if data.len() % 2 != 0 {
        return Err(AppError::Audio(format!(
            "raw s16le audio is truncated ({} bytes)",
            data.len()
        )));
    }
    Ok(data
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / i16::MAX as f32)
        .collect())
}

/// Convenience: normalize `input` through ffmpeg and decode it.
/// Converts raw PCM directly, avoiding a simultaneous synthetic WAV copy.
pub fn load_samples(input: &Path) -> Result<Vec<f32>> {
    let pcm = normalize_to_memory(input)?;
    decode_pcm_s16le(&pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    // WP-116 DoD 1: ffmpeg already emits canonical s16le, so conversion can
    // consume those raw bytes directly without allocating a synthetic WAV.
    // Expected values intentionally preserve the existing hound path's
    // `i16::MAX` normalization contract.
    #[test]
    fn decode_pcm_s16le_converts_raw_little_endian_samples_directly() {
        let source = [i16::MIN, -16_384, 0, 16_384, i16::MAX];
        let bytes: Vec<u8> = source
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();

        let decoded = decode_pcm_s16le(&bytes).expect("valid raw s16le decodes");

        let expected: Vec<f32> = source
            .iter()
            .map(|sample| *sample as f32 / i16::MAX as f32)
            .collect();
        assert_eq!(decoded, expected);
    }

    #[test]
    fn decode_pcm_s16le_rejects_a_trailing_odd_byte() {
        let error = decode_pcm_s16le(&[0x00, 0x80, 0xff])
            .expect_err("an incomplete i16 sample must not be truncated silently");

        assert!(matches!(error, AppError::Audio(_)));
    }
}
