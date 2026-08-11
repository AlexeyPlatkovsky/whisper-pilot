//! Rust-owned diarization pipeline: segmentation inference, speaker
//! embedding, embedding clustering, and turn reconstruction from window
//! assignments.

use crate::diarize::clustering::{cluster_embeddings, ClusterStop};
use crate::diarize::segmentation::SegmentationWindow;
use crate::diarize::segmentation::{embedding_progress_units, DirectSegmentationModel};
use crate::error::{AppError, Result};
use sherpa_rs::speaker_id::{EmbeddingExtractor, ExtractorConfig};
use std::collections::BTreeMap;

use super::speakers::SpeakerTurn;
use super::{DiarizationModelPaths, ProgressCallback};

const MIN_EMBEDDING_FRAMES: usize = 10;
const MIN_TURN_DURATION_MS: u32 = 300;
const MAX_SPEAKER_GAP_MS: u32 = 500;

pub(crate) fn diarize_with_rust_clustering(
    models: &DiarizationModelPaths,
    samples: Vec<f32>,
    speaker_count: Option<i32>,
    threshold: f32,
    mut progress: Option<ProgressCallback>,
) -> Result<Vec<SpeakerTurn>> {
    if speaker_count.filter(|count| *count > 0).is_some() {
        return Err(AppError::Diarization(
            "fixed speaker-count diarization is reserved for WP-49".to_string(),
        ));
    }

    let model = DirectSegmentationModel::load(&models.segmentation_model)?;
    if model.metadata.sample_rate != 16_000 {
        return Err(AppError::Diarization(format!(
            "segmentation model expects {} Hz audio, not WhisperPilot's 16000 Hz samples",
            model.metadata.sample_rate
        )));
    }
    let receptive_field_shift = model.metadata.receptive_field_shift;
    let windows = model.infer(&samples, &mut progress)?;
    if windows.is_empty() {
        return Ok(Vec::new());
    }

    let mut extractor = EmbeddingExtractor::new(ExtractorConfig {
        model: models.embedding_model.to_string_lossy().to_string(),
        num_threads: Some(1),
        ..Default::default()
    })
    .map_err(|error| {
        AppError::Diarization(format!("could not initialize embedding extractor: {error}"))
    })?;

    let mut pairs = Vec::new();
    let mut embeddings = Vec::new();
    for (window_index, window) in windows.iter().enumerate() {
        let local_speakers = window.activity.first().map_or(0, Vec::len);
        for local_speaker in 0..local_speakers {
            let active_frames = window
                .activity
                .iter()
                .filter(|activity| activity[local_speaker])
                .count();
            if active_frames < MIN_EMBEDDING_FRAMES {
                continue;
            }
            let audio =
                masked_window_audio(&samples, window, local_speaker, model.metadata.window_size);
            if audio.is_empty() {
                continue;
            }
            let embedding = extractor
                .compute_speaker_embedding(audio, model.metadata.sample_rate)
                .map_err(|error| {
                    AppError::Diarization(format!("could not compute speaker embedding: {error}"))
                })?;
            pairs.push((window_index, local_speaker));
            embeddings.push(embedding);
        }
        if let Some(callback) = progress.as_ref() {
            let (done, total) = embedding_progress_units(window_index + 1, windows.len())?;
            callback(done, total);
        }
    }
    if embeddings.is_empty() {
        return Ok(Vec::new());
    }

    let labels = cluster_embeddings(&embeddings, ClusterStop::Distance(threshold))?;
    let assignments: BTreeMap<(usize, usize), usize> = pairs.into_iter().zip(labels).collect();
    turns_from_assignments(
        &windows,
        &assignments,
        samples.len(),
        receptive_field_shift,
        model.metadata.sample_rate,
    )
}

fn masked_window_audio(
    samples: &[f32],
    window: &SegmentationWindow,
    local_speaker: usize,
    window_size: usize,
) -> Vec<f32> {
    let frame_count = window.activity.len();
    let mut audio = Vec::new();
    for (frame, activity) in window.activity.iter().enumerate() {
        if !activity[local_speaker] {
            continue;
        }
        let start = window.start_sample + frame * window_size / frame_count;
        let end = window.start_sample + (frame + 1) * window_size / frame_count;
        if start < samples.len() {
            audio.extend_from_slice(&samples[start..end.min(samples.len())]);
        }
    }
    audio
}

fn turns_from_assignments(
    windows: &[SegmentationWindow],
    assignments: &BTreeMap<(usize, usize), usize>,
    sample_len: usize,
    receptive_field_shift: usize,
    sample_rate: u32,
) -> Result<Vec<SpeakerTurn>> {
    if receptive_field_shift == 0 || sample_rate == 0 {
        return Err(AppError::Diarization(
            "segmentation metadata has a zero frame shift or sample rate".to_string(),
        ));
    }
    if assignments.is_empty() {
        return Ok(Vec::new());
    }
    let frames = sample_len.div_ceil(receptive_field_shift);
    let cluster_count = assignments.values().copied().max().unwrap_or(0) + 1;
    let mut votes = vec![vec![0u32; cluster_count]; frames];
    let mut local_speaker_counts = vec![0usize; frames];
    let mut window_counts = vec![0usize; frames];
    for (window_index, window) in windows.iter().enumerate() {
        let base = (window.start_sample as f64 / receptive_field_shift as f64).round() as usize;
        for (frame, activity) in window.activity.iter().enumerate() {
            let target = base + frame;
            if target >= votes.len() {
                break;
            }
            local_speaker_counts[target] += activity.iter().filter(|active| **active).count();
            window_counts[target] += 1;
            for (local_speaker, active) in activity.iter().enumerate() {
                if *active {
                    if let Some(cluster) = assignments.get(&(window_index, local_speaker)) {
                        votes[target][*cluster] += 1;
                    }
                }
            }
        }
    }

    let mut activity_by_cluster = vec![vec![false; frames]; cluster_count];
    for frame in 0..frames {
        let window_count = window_counts[frame];
        if window_count == 0 {
            continue;
        }
        let speaker_count = (local_speaker_counts[frame] * 2 + window_count) / (window_count * 2);
        let mut ranked: Vec<(usize, u32)> = votes[frame]
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, count)| *count > 0)
            .collect();
        ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        for (cluster, _) in ranked.into_iter().take(speaker_count) {
            activity_by_cluster[cluster][frame] = true;
        }
    }

    let mut turns = Vec::new();
    for (speaker, activity) in activity_by_cluster.iter_mut().enumerate() {
        smooth_speaker_activity(activity, receptive_field_shift, sample_rate);
        let mut start = 0;
        while start < activity.len() {
            if !activity[start] {
                start += 1;
                continue;
            }
            let mut end = start + 1;
            while end < activity.len() && activity[end] {
                end += 1;
            }
            turns.push(SpeakerTurn {
                start_ms: ((start * receptive_field_shift * 1_000) / sample_rate as usize) as u32,
                end_ms: ((end * receptive_field_shift * 1_000) / sample_rate as usize) as u32,
                speaker: speaker as i32,
            });
            start = end;
        }
    }
    turns.sort_by_key(|turn| turn.start_ms);
    Ok(turns)
}

fn smooth_speaker_activity(activity: &mut [bool], receptive_field_shift: usize, sample_rate: u32) {
    let duration_ms = |frames: usize| -> u32 {
        ((frames * receptive_field_shift * 1_000) / sample_rate as usize) as u32
    };
    let mut index = 0;
    while index < activity.len() {
        if activity[index] {
            index += 1;
            continue;
        }
        let start = index;
        while index < activity.len() && !activity[index] {
            index += 1;
        }
        if start > 0 && index < activity.len() && duration_ms(index - start) <= MAX_SPEAKER_GAP_MS {
            activity[start..index].fill(true);
        }
    }

    let mut index = 0;
    while index < activity.len() {
        if !activity[index] {
            index += 1;
            continue;
        }
        let start = index;
        while index < activity.len() && activity[index] {
            index += 1;
        }
        if duration_ms(index - start) < MIN_TURN_DURATION_MS {
            activity[start..index].fill(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speaker_turn(start_ms: u32, end_ms: u32, speaker: i32) -> SpeakerTurn {
        SpeakerTurn {
            start_ms,
            end_ms,
            speaker,
        }
    }

    #[test]
    fn wp67_turn_reconstruction_merges_short_gaps_and_drops_short_runs() {
        // BVA: a 200ms gap must merge; a 200ms competing run must be dropped.
        let windows = [SegmentationWindow {
            start_sample: 0,
            activity: vec![
                vec![true, false],
                vec![true, false],
                vec![false, true],
                vec![false, true],
                vec![true, false],
                vec![true, false],
                vec![true, false],
                vec![true, false],
                vec![true, false],
                vec![true, false],
            ],
        }];
        let assignments = BTreeMap::from([((0, 0), 0), ((0, 1), 1)]);

        assert_eq!(
            turns_from_assignments(&windows, &assignments, 1_000, 100, 1_000).unwrap(),
            vec![speaker_turn(0, 1_000, 0)]
        );
    }

    #[test]
    fn wp67_turn_reconstruction_keeps_overlapping_speakers() {
        // EP: a frame with two local speakers must retain both global labels.
        let windows = [SegmentationWindow {
            start_sample: 0,
            activity: vec![vec![true, true]; 5],
        }];
        let assignments = BTreeMap::from([((0, 0), 0), ((0, 1), 1)]);

        assert_eq!(
            turns_from_assignments(&windows, &assignments, 500, 100, 1_000).unwrap(),
            vec![speaker_turn(0, 500, 0), speaker_turn(0, 500, 1),]
        );
    }
}
