//! The transport format between the parent process and the diarization
//! worker: a JSON request file, a raw little-endian `f32` samples file, and a
//! JSON turns output file. See ADR-013 for why samples cross the process
//! boundary as a file rather than a pipe.

use crate::diarize::SpeakerTurn;
use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, Read, Write};
use std::path::PathBuf;

const SAMPLE_WRITE_BUFFER_BYTES: usize = 16 * 1_024;

/// Everything the worker needs, handed over as a small JSON file rather than
/// argv so paths with spaces and a growing field set stay uncomplicated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerRequest {
    pub app_support_dir: PathBuf,
    pub samples_path: PathBuf,
    pub output_path: PathBuf,
    pub variant: String,
    pub speaker_count: Option<i32>,
}

/// Samples cross the process boundary as a raw little-endian `f32` file rather
/// than a pipe — see ADR-013 for why.
pub(crate) fn write_samples(path: &std::path::Path, samples: &[f32]) -> Result<()> {
    let mut file = std::fs::File::create(path).map_err(|e| {
        AppError::Diarization(format!(
            "could not stage audio for the speaker-identification process at {}: {e}",
            path.display()
        ))
    })?;
    write_samples_to(&mut file, samples)
}

pub(crate) fn write_samples_to(writer: &mut impl Write, samples: &[f32]) -> Result<()> {
    let mut bytes = [0_u8; SAMPLE_WRITE_BUFFER_BYTES];
    let samples_per_chunk = SAMPLE_WRITE_BUFFER_BYTES / std::mem::size_of::<f32>();
    for chunk in samples.chunks(samples_per_chunk) {
        for (index, sample) in chunk.iter().enumerate() {
            let start = index * std::mem::size_of::<f32>();
            bytes[start..start + 4].copy_from_slice(&sample.to_le_bytes());
        }
        writer
            .write_all(&bytes[..std::mem::size_of_val(chunk)])
            .map_err(|error| {
                AppError::Diarization(format!("could not stage speaker audio: {error}"))
            })?;
    }
    writer.flush().map_err(|error| {
        AppError::Diarization(format!("could not flush staged speaker audio: {error}"))
    })
}

pub(crate) fn read_samples(path: &std::path::Path) -> Result<Vec<f32>> {
    let file = std::fs::File::open(path).map_err(|e| {
        AppError::Diarization(format!(
            "could not read staged audio at {}: {e}",
            path.display()
        ))
    })?;
    let byte_len = file
        .metadata()
        .map_err(|e| {
            AppError::Diarization(format!(
                "could not inspect staged audio at {}: {e}",
                path.display()
            ))
        })?
        .len();
    if byte_len % std::mem::size_of::<f32>() as u64 != 0 {
        return Err(AppError::Diarization(format!(
            "staged audio at {} is truncated ({} bytes is not a whole number of samples)",
            path.display(),
            byte_len
        )));
    }

    let byte_len = usize::try_from(byte_len)
        .map_err(|_| AppError::Diarization("staged audio is too large to address".to_string()))?;
    let sample_count = byte_len / std::mem::size_of::<f32>();
    let mut samples = Vec::with_capacity(sample_count);
    let mut reader = BufReader::new(file);
    let mut bytes = [0_u8; SAMPLE_WRITE_BUFFER_BYTES];
    let mut remaining = byte_len;
    while remaining > 0 {
        let chunk_len = remaining.min(bytes.len());
        reader.read_exact(&mut bytes[..chunk_len]).map_err(|e| {
            AppError::Diarization(format!(
                "could not read staged audio at {}: {e}",
                path.display()
            ))
        })?;
        samples.extend(
            bytes[..chunk_len]
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])),
        );
        remaining -= chunk_len;
    }
    Ok(samples)
}

/// Read the worker's turns output JSON. A clean exit is only a success when
/// it also left a readable payload.
pub(crate) fn read_turns(path: &std::path::Path) -> Result<Vec<SpeakerTurn>> {
    let bytes = std::fs::read(path).map_err(|e| {
        AppError::Diarization(format!(
            "the engine exited cleanly but left no result at {}: {e}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Diarization(format!("the engine's result was unreadable: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[derive(Default)]
    struct RecordingWriter {
        bytes: Vec<u8>,
        writes: usize,
        max_write_size: usize,
    }

    impl Write for RecordingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.writes += 1;
            self.max_write_size = self.max_write_size.max(bytes.len());
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn worker_request_round_trips() {
        let original = WorkerRequest {
            app_support_dir: std::path::PathBuf::from("/support"),
            samples_path: std::path::PathBuf::from("/cache/samples.f32"),
            output_path: std::path::PathBuf::from("/cache/turns.json"),
            variant: "titanet-large".to_string(),
            speaker_count: Some(2),
        };

        let json = serde_json::to_string(&original).unwrap();
        let round_tripped: WorkerRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(round_tripped, original);
    }

    #[test]
    fn samples_round_trip_through_the_transport_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("samples.f32");
        let samples = vec![0.0_f32, -1.0, 0.5, f32::MIN_POSITIVE];

        write_samples(&path, &samples).unwrap();

        assert_eq!(read_samples(&path).unwrap(), samples);
    }

    #[test]
    fn reading_a_truncated_samples_file_is_an_error_not_a_silent_short_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("truncated.f32");
        // 6 bytes: one whole f32 plus half of another.
        std::fs::write(&path, [0u8, 0, 0, 0, 1, 2]).unwrap();

        let error = read_samples(&path).expect_err("a partial sample must not be silently dropped");

        assert!(matches!(error, crate::error::AppError::Diarization(_)));
    }

    // WP-116 DoD 1: staging must serialize through an injected sink in fixed
    // increments, never first allocate one Vec<u8> proportional to the full
    // recording. Exact bytes also pin the existing little-endian wire format.
    #[test]
    fn sample_staging_writes_exact_little_endian_bytes_in_bounded_chunks() {
        let samples: Vec<f32> = (0..20_000)
            .map(|index| (index as f32 - 10_000.0) / 10_000.0)
            .collect();
        let expected: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let mut writer = RecordingWriter::default();

        write_samples_to(&mut writer, &samples).expect("model-free sample staging succeeds");

        assert_eq!(writer.bytes, expected);
        assert!(writer.writes > 1, "a full recording must not be one write");
        assert!(
            writer.max_write_size <= 16 * 1_024,
            "largest write was {} bytes; staging must use a fixed small buffer",
            writer.max_write_size
        );
    }
}
