//! Speaker-embedding clustering (WP-62 Rust-owned path). Deterministic
//! incremental-centroid threshold clustering over normalized embeddings.

use crate::error::{AppError, Result};

/// The stopping rule for Rust-owned speaker clustering.
///
/// WP-62 ships the distance-threshold mode. The fixed-count variant remains
/// represented here so WP-49 can add its speaker-count override without
/// redesigning the public clustering contract.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClusterStop {
    /// Stop merging once the nearest pair is farther apart than this cosine
    /// distance. Valid values are in the inclusive cosine-distance range 0–2.
    Distance(f32),
    /// Reserved for WP-49; deliberately not implemented in WP-62.
    FixedCount(usize),
}

#[derive(Debug)]
struct EmbeddingCluster {
    members: Vec<usize>,
    centroid: Vec<f32>,
}

/// Deterministic incremental-centroid threshold clustering for normalized
/// embeddings. Labels follow first input occurrence; see Speaker Diarization
/// in `docs/architecture.md` for the production-path constraint.
pub fn cluster_embeddings(embeddings: &[Vec<f32>], stop: ClusterStop) -> Result<Vec<usize>> {
    let threshold = match stop {
        ClusterStop::Distance(value) if value.is_finite() && (0.0..=2.0).contains(&value) => value,
        ClusterStop::Distance(value) => {
            return Err(AppError::Diarization(format!(
                "invalid cosine-distance threshold {value}; expected a finite value from 0 to 2"
            )));
        }
        ClusterStop::FixedCount(_) => {
            return Err(AppError::Diarization(
                "fixed-count speaker clustering is reserved for WP-49".to_string(),
            ));
        }
    };

    if embeddings.is_empty() {
        return Ok(Vec::new());
    }

    let dimension = embeddings[0].len();
    if dimension == 0 {
        return Err(AppError::Diarization(
            "speaker embedding must have at least one dimension".to_string(),
        ));
    }

    let mut clusters: Vec<EmbeddingCluster> = Vec::new();
    for (index, embedding) in embeddings.iter().enumerate() {
        if embedding.len() != dimension {
            return Err(AppError::Diarization(format!(
                "speaker embedding {index} has dimension {}; expected {dimension}",
                embedding.len()
            )));
        }
        let normalized = normalize_embedding(embedding, index)?;
        let nearest = clusters
            .iter()
            .enumerate()
            .map(|(cluster_index, cluster)| {
                (
                    cluster_index,
                    cosine_distance(&normalized, &cluster.centroid),
                )
            })
            .min_by(|left, right| left.1.total_cmp(&right.1));

        if let Some((cluster_index, distance)) = nearest {
            if distance <= threshold {
                let cluster = &mut clusters[cluster_index];
                cluster.centroid = weighted_normalized_centroid(
                    &cluster.centroid,
                    cluster.members.len(),
                    &normalized,
                )?;
                cluster.members.push(index);
                continue;
            }
        }

        clusters.push(EmbeddingCluster {
            members: vec![index],
            centroid: normalized,
        });
    }

    let mut labels = vec![0; embeddings.len()];
    for (label, cluster) in clusters.iter().enumerate() {
        for &member in &cluster.members {
            labels[member] = label;
        }
    }
    Ok(labels)
}

fn normalize_embedding(embedding: &[f32], index: usize) -> Result<Vec<f32>> {
    if embedding.iter().any(|value| !value.is_finite()) {
        return Err(AppError::Diarization(format!(
            "speaker embedding {index} contains a non-finite value"
        )));
    }
    let norm_squared: f32 = embedding.iter().map(|value| value * value).sum();
    if !norm_squared.is_finite() || norm_squared <= f32::EPSILON {
        return Err(AppError::Diarization(format!(
            "speaker embedding {index} has zero magnitude"
        )));
    }
    let norm = norm_squared.sqrt();
    Ok(embedding.iter().map(|value| value / norm).collect())
}

fn weighted_normalized_centroid(
    centroid: &[f32],
    centroid_members: usize,
    embedding: &[f32],
) -> Result<Vec<f32>> {
    let member_count = centroid_members as f32;
    let mean: Vec<f32> = centroid
        .iter()
        .zip(embedding)
        .map(|(centroid_value, embedding_value)| {
            (centroid_value * member_count + embedding_value) / (member_count + 1.0)
        })
        .collect();
    normalize_embedding(&mean, 0).map_err(|_| {
        AppError::Diarization("cannot merge speaker clusters with opposite centroids".to_string())
    })
}

fn cosine_distance(left: &[f32], right: &[f32]) -> f32 {
    let similarity: f32 = left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum();
    1.0 - similarity.clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wp66_numerical_helpers_reject_invalid_vectors_and_preserve_cosine_bounds() {
        // EP: non-finite and zero vectors are invalid embedding classes.
        assert!(matches!(
            normalize_embedding(&[f32::NAN, 1.0], 0),
            Err(AppError::Diarization(_))
        ));
        assert!(matches!(
            normalize_embedding(&[0.0, 0.0], 0),
            Err(AppError::Diarization(_))
        ));
        assert!(matches!(
            weighted_normalized_centroid(&[1.0, 0.0], 1, &[-1.0, 0.0]),
            Err(AppError::Diarization(_))
        ));

        // BVA: identical, orthogonal, and opposite unit vectors bound cosine distance.
        assert_eq!(cosine_distance(&[1.0, 0.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine_distance(&[1.0, 0.0], &[0.0, 1.0]), 1.0);
        assert_eq!(cosine_distance(&[1.0, 0.0], &[-1.0, 0.0]), 2.0);
    }
}
