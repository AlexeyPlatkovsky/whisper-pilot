//! Recoverable native-rate Recorder audio in Core Audio Format (CAF).
//!
//! The app keeps one mono signed-PCM16 master at the input device's native
//! sample rate. While capture is active the file ends in `.caf.partial` and
//! uses CAF's legal unknown data-chunk size. Finalization patches the exact
//! size, syncs and closes the file before atomically promoting it to `.caf`.

use crate::error::{AppError, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const CAF_HEADER_LEN: u64 = 68;
const DATA_CHUNK_SIZE_OFFSET: u64 = 56;
const PCM_BYTES_PER_SAMPLE: u64 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderAudioMetadata {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub frames: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderAudio {
    pub metadata: RecorderAudioMetadata,
    pub samples: Vec<i16>,
}

pub struct RecorderAudioWriter {
    final_path: PathBuf,
    partial_path: PathBuf,
    file: Option<File>,
    sample_rate: u32,
    frames: u64,
    unsynced_samples: u64,
    checkpoint_count: u64,
}

impl RecorderAudioWriter {
    pub fn partial_path_for(final_path: &Path) -> PathBuf {
        let mut value = final_path.as_os_str().to_os_string();
        value.push(".partial");
        PathBuf::from(value)
    }

    pub fn create(final_path: &Path, sample_rate: u32) -> Result<Self> {
        if sample_rate == 0 {
            return Err(AppError::Audio(
                "Recorder sample rate must be greater than zero".into(),
            ));
        }
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let partial_path = Self::partial_path_for(final_path);
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&partial_path)?;
        write_header(&mut file, sample_rate, -1)?;
        file.sync_data()?;
        Ok(Self {
            final_path: final_path.to_path_buf(),
            partial_path,
            file: Some(file),
            sample_rate,
            frames: 0,
            unsynced_samples: 0,
            checkpoint_count: 1,
        })
    }

    /// Continue a finalized recording without mutating its durable audio in
    /// place. The existing PCM is copied into a new `.partial` CAF and new
    /// samples append there; `finalize` atomically replaces the old final file.
    pub fn resume(final_path: &Path, sample_rate: u32) -> Result<Self> {
        if sample_rate == 0 {
            return Err(AppError::Audio(
                "Recorder sample rate must be greater than zero".into(),
            ));
        }
        let partial_path = Self::partial_path_for(final_path);
        if partial_path.exists() {
            return Err(AppError::Audio(
                "Recorder continuation found an existing partial audio file".into(),
            ));
        }

        let mut source = File::open(final_path)?;
        let (metadata, pcm_bytes) = inspect_caf(&mut source, false)?;
        if metadata.sample_rate != sample_rate {
            return Err(AppError::Audio(format!(
                "Recorder continuation sample rate {sample_rate} does not match existing audio rate {}",
                metadata.sample_rate
            )));
        }
        let mut partial = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&partial_path)?;
        let copied = (|| -> Result<u64> {
            write_header(&mut partial, sample_rate, -1)?;
            let copied = std::io::copy(&mut source.take(pcm_bytes), &mut partial)?;
            if copied != pcm_bytes {
                return Err(AppError::Audio(
                    "Recorder continuation could not copy the complete existing audio".into(),
                ));
            }
            partial.flush()?;
            partial.sync_data()?;
            Ok(copied)
        })();
        if let Err(error) = copied {
            drop(partial);
            let _ = std::fs::remove_file(&partial_path);
            return Err(error);
        }

        Ok(Self {
            final_path: final_path.to_path_buf(),
            partial_path,
            file: Some(partial),
            sample_rate,
            frames: metadata.frames,
            unsynced_samples: 0,
            checkpoint_count: 1,
        })
    }

    pub fn append_f32(&mut self, samples: &[f32]) -> Result<()> {
        let mut offset = 0;
        while offset < samples.len() {
            let checkpoint_remaining =
                u64::from(self.sample_rate).saturating_sub(self.unsynced_samples);
            if checkpoint_remaining == 0 {
                self.sync_checkpoint()?;
                continue;
            }
            let take = usize::try_from(checkpoint_remaining)
                .unwrap_or(usize::MAX)
                .min(samples.len() - offset);
            let mut pcm = Vec::with_capacity(take * 2);
            for sample in &samples[offset..offset + take] {
                let value = if *sample <= -1.0 {
                    i16::MIN
                } else if *sample >= 1.0 {
                    i16::MAX
                } else {
                    (*sample * 32_768.0).round() as i16
                };
                pcm.extend_from_slice(&value.to_le_bytes());
            }
            self.file_mut()?.write_all(&pcm)?;
            self.frames = self.frames.saturating_add(take as u64);
            self.unsynced_samples = self.unsynced_samples.saturating_add(take as u64);
            offset += take;
            if self.unsynced_samples == u64::from(self.sample_rate) {
                self.sync_checkpoint()?;
            }
        }
        Ok(())
    }

    pub fn sync_checkpoint(&mut self) -> Result<()> {
        self.file_mut()?.flush()?;
        self.file_mut()?.sync_data()?;
        self.unsynced_samples = 0;
        self.checkpoint_count = self.checkpoint_count.saturating_add(1);
        Ok(())
    }

    pub fn checkpoint_count(&self) -> u64 {
        self.checkpoint_count
    }

    pub fn unsynced_samples(&self) -> u64 {
        self.unsynced_samples
    }

    pub fn duration_ms(&self) -> u64 {
        self.frames.saturating_mul(1_000) / u64::from(self.sample_rate)
    }

    pub fn finalize(mut self) -> Result<PathBuf> {
        let data_chunk_size = 4_u64
            .checked_add(self.frames.saturating_mul(PCM_BYTES_PER_SAMPLE))
            .ok_or_else(|| AppError::Audio("Recorder CAF data length overflowed".into()))?;
        let mut file = self
            .file
            .take()
            .ok_or_else(|| AppError::Audio("Recorder audio is already closed".into()))?;
        file.flush()?;
        file.seek(SeekFrom::Start(DATA_CHUNK_SIZE_OFFSET))?;
        file.write_all(&(data_chunk_size as i64).to_be_bytes())?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&self.partial_path, &self.final_path)?;
        if let Some(parent) = self.final_path.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(self.final_path)
    }

    fn file_mut(&mut self) -> Result<&mut File> {
        self.file
            .as_mut()
            .ok_or_else(|| AppError::Audio("Recorder audio is already closed".into()))
    }
}

fn write_header(file: &mut File, sample_rate: u32, data_chunk_size: i64) -> Result<()> {
    file.write_all(b"caff")?;
    file.write_all(&1_u16.to_be_bytes())?;
    file.write_all(&0_u16.to_be_bytes())?;
    file.write_all(b"desc")?;
    file.write_all(&32_i64.to_be_bytes())?;
    file.write_all(&(sample_rate as f64).to_bits().to_be_bytes())?;
    file.write_all(b"lpcm")?;
    file.write_all(&2_u32.to_be_bytes())?; // signed integer, little-endian PCM
    file.write_all(&2_u32.to_be_bytes())?; // bytes per packet
    file.write_all(&1_u32.to_be_bytes())?; // frames per packet
    file.write_all(&1_u32.to_be_bytes())?; // mono
    file.write_all(&16_u32.to_be_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_chunk_size.to_be_bytes())?;
    file.write_all(&0_u32.to_be_bytes())?; // edit count
    Ok(())
}

pub fn read_caf_audio(path: &Path) -> Result<RecorderAudio> {
    let mut file = File::open(path)?;
    let (metadata, pcm_bytes) = inspect_caf(&mut file, false)?;
    let mut bytes = vec![0_u8; pcm_bytes as usize];
    file.read_exact(&mut bytes)?;
    let samples = bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    Ok(RecorderAudio { metadata, samples })
}

pub fn read_caf_metadata(path: &Path) -> Result<RecorderAudioMetadata> {
    let mut file = File::open(path)?;
    inspect_caf(&mut file, false).map(|(metadata, _)| metadata)
}

fn inspect_caf(
    file: &mut File,
    allow_unknown_data_size: bool,
) -> Result<(RecorderAudioMetadata, u64)> {
    let mut header = [0_u8; CAF_HEADER_LEN as usize];
    file.read_exact(&mut header)?;
    if &header[0..4] != b"caff"
        || u16::from_be_bytes(header[4..6].try_into().unwrap()) != 1
        || &header[8..12] != b"desc"
        || i64::from_be_bytes(header[12..20].try_into().unwrap()) != 32
        || &header[28..32] != b"lpcm"
        || &header[52..56] != b"data"
    {
        return Err(AppError::Audio(
            "unsupported or invalid Recorder CAF file".into(),
        ));
    }
    let sample_rate = f64::from_bits(u64::from_be_bytes(header[20..28].try_into().unwrap()));
    let flags = u32::from_be_bytes(header[32..36].try_into().unwrap());
    let bytes_per_packet = u32::from_be_bytes(header[36..40].try_into().unwrap());
    let frames_per_packet = u32::from_be_bytes(header[40..44].try_into().unwrap());
    let channels = u32::from_be_bytes(header[44..48].try_into().unwrap());
    let bits = u32::from_be_bytes(header[48..52].try_into().unwrap());
    if !sample_rate.is_finite()
        || sample_rate <= 0.0
        || sample_rate > u32::MAX as f64
        || flags != 2
        || bytes_per_packet != 2
        || frames_per_packet != 1
        || channels != 1
        || bits != 16
    {
        return Err(AppError::Audio(
            "unsupported Recorder CAF audio description".into(),
        ));
    }
    let declared_size = i64::from_be_bytes(header[56..64].try_into().unwrap());
    let file_len = file.metadata()?.len();
    let available_pcm = file_len.saturating_sub(CAF_HEADER_LEN);
    let pcm_bytes = if declared_size == -1 && allow_unknown_data_size {
        available_pcm
    } else if declared_size < 4 {
        return Err(AppError::Audio(
            "Recorder CAF has an invalid data chunk size".into(),
        ));
    } else {
        let declared_pcm = declared_size as u64 - 4;
        if declared_pcm != available_pcm {
            return Err(AppError::Audio(
                "Recorder CAF data chunk does not match the file length".into(),
            ));
        }
        declared_pcm
    };
    if pcm_bytes % PCM_BYTES_PER_SAMPLE != 0 {
        return Err(AppError::Audio(
            "Recorder CAF contains a truncated PCM sample".into(),
        ));
    }
    Ok((
        RecorderAudioMetadata {
            sample_rate: sample_rate.round() as u32,
            channels: 1,
            bits_per_sample: 16,
            frames: pcm_bytes / PCM_BYTES_PER_SAMPLE,
        },
        pcm_bytes,
    ))
}

pub fn export_caf_to_wav(caf_path: &Path, wav_path: &Path) -> Result<()> {
    let mut caf = File::open(caf_path)?;
    let (metadata, pcm_bytes) = inspect_caf(&mut caf, false)?;
    let spec = hound::WavSpec {
        channels: metadata.channels,
        sample_rate: metadata.sample_rate,
        bits_per_sample: metadata.bits_per_sample,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(wav_path, spec)
        .map_err(|error| AppError::Audio(error.to_string()))?;
    let mut remaining = pcm_bytes;
    let mut buffer = [0_u8; 8_192];
    while remaining > 0 {
        let take = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        caf.read_exact(&mut buffer[..take])?;
        for pair in buffer[..take].chunks_exact(2) {
            writer
                .write_sample(i16::from_le_bytes([pair[0], pair[1]]))
                .map_err(|error| AppError::Audio(error.to_string()))?;
        }
        remaining -= take as u64;
    }
    writer
        .finalize()
        .map_err(|error| AppError::Audio(error.to_string()))
}

/// Promotes a structurally valid interrupted `.caf.partial` without
/// rewriting its PCM payload. The exact data size is derived from the file
/// length, patched and synced before the same-directory atomic rename.
pub fn finalize_partial_caf(final_path: &Path) -> Result<PathBuf> {
    let partial_path = RecorderAudioWriter::partial_path_for(final_path);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&partial_path)?;
    file.seek(SeekFrom::Start(DATA_CHUNK_SIZE_OFFSET))?;
    let mut declared_size = [0_u8; 8];
    file.read_exact(&mut declared_size)?;
    if i64::from_be_bytes(declared_size) != -1 {
        return Err(AppError::Audio(
            "Recorder recovery expected an unfinished partial CAF".into(),
        ));
    }
    file.seek(SeekFrom::Start(0))?;
    let _ = inspect_caf(&mut file, true)?;
    let len = file.metadata()?.len();
    if len < CAF_HEADER_LEN || (len - CAF_HEADER_LEN) % PCM_BYTES_PER_SAMPLE != 0 {
        return Err(AppError::Audio(
            "interrupted Recorder CAF is truncated".into(),
        ));
    }
    let data_chunk_size = 4_u64.saturating_add(len - CAF_HEADER_LEN);
    file.seek(SeekFrom::Start(DATA_CHUNK_SIZE_OFFSET))?;
    file.write_all(&(data_chunk_size as i64).to_be_bytes())?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    let _ = read_caf_metadata(&partial_path)?;
    std::fs::rename(&partial_path, final_path)?;
    if let Some(parent) = final_path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(final_path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::{read_caf_audio, RecorderAudioWriter};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn completed_caf(path: &Path, sample_rate: u32, samples: &[f32]) {
        let mut writer = RecorderAudioWriter::create(path, sample_rate).unwrap();
        writer.append_f32(samples).unwrap();
        writer.finalize().unwrap();
    }

    #[test]
    fn resuming_is_copy_on_write_until_combined_audio_is_atomically_finalized() {
        let temp = TempDir::new().unwrap();
        let final_path = temp.path().join("continued.caf");
        completed_caf(&final_path, 48_000, &[0.25; 4_800]);
        let original_bytes = fs::read(&final_path).unwrap();

        let mut writer = RecorderAudioWriter::resume(&final_path, 48_000).unwrap();
        writer.append_f32(&[-0.25; 2_400]).unwrap();

        assert_eq!(fs::read(&final_path).unwrap(), original_bytes);
        assert_eq!(read_caf_audio(&final_path).unwrap().samples.len(), 4_800);

        writer.finalize().unwrap();

        let combined = read_caf_audio(&final_path).unwrap();
        assert_eq!(combined.metadata.frames, 7_200);
        assert_eq!(&combined.samples[..4_800], &[8_192; 4_800]);
        assert_eq!(&combined.samples[4_800..], &[-8_192; 2_400]);
        assert!(!RecorderAudioWriter::partial_path_for(&final_path).exists());
    }

    #[test]
    fn resume_rejects_a_sample_rate_mismatch_without_touching_the_completed_caf() {
        let temp = TempDir::new().unwrap();
        let final_path = temp.path().join("rate-mismatch.caf");
        completed_caf(&final_path, 48_000, &[0.125; 480]);
        let original_bytes = fs::read(&final_path).unwrap();

        let result = RecorderAudioWriter::resume(&final_path, 44_100);

        assert!(result.is_err());
        assert_eq!(fs::read(&final_path).unwrap(), original_bytes);
        assert!(!RecorderAudioWriter::partial_path_for(&final_path).exists());
    }

    #[test]
    fn failed_append_cannot_corrupt_the_previous_completed_caf() {
        let temp = TempDir::new().unwrap();
        let final_path = temp.path().join("failed-append.caf");
        completed_caf(&final_path, 48_000, &[0.5; 480]);
        let original_bytes = fs::read(&final_path).unwrap();
        let mut writer = RecorderAudioWriter::resume(&final_path, 48_000).unwrap();

        drop(writer.file.take());
        assert!(writer.append_f32(&[-0.5; 480]).is_err());

        assert_eq!(fs::read(&final_path).unwrap(), original_bytes);
        assert_eq!(read_caf_audio(&final_path).unwrap().samples, &[16_384; 480]);
    }
}
