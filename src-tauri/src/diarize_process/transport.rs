//! The transport format between the parent process and the diarization
//! worker: a JSON request file, a raw little-endian `f32` samples file, and a
//! JSON turns output file. See ADR-013 for why samples cross the process
//! boundary as a file rather than a pipe.

use crate::diarize::SpeakerTurn;
use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).map_err(|e| {
        AppError::Diarization(format!(
            "could not stage audio for the speaker-identification process at {}: {e}",
            path.display()
        ))
    })
}

pub(crate) fn read_samples(path: &std::path::Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path).map_err(|e| {
        AppError::Diarization(format!(
            "could not read staged audio at {}: {e}",
            path.display()
        ))
    })?;
    if bytes.len() % 4 != 0 {
        // `chunks_exact` would drop the partial tail silently, diarizing a
        // slightly different recording than the one transcribed.
        return Err(AppError::Diarization(format!(
            "staged audio at {} is truncated ({} bytes is not a whole number of samples)",
            path.display(),
            bytes.len()
        )));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
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
}
