//! WP-62 TDD coverage for Rust-owned agglomerative clustering.
//!
//! These tests intentionally exercise only the pure clustering boundary: model
//! loading and ONNX inference remain behind the diarization worker boundary.

use whisperpilot_lib::diarize::{
    cluster_embeddings, powerset_class_to_activity, segmentation_windows, ClusterStop,
};

#[test]
fn s3_empty_and_single_speaker_inputs_are_deterministic() {
    // EP: no embedding and one-speaker embedding classes.
    assert_eq!(
        cluster_embeddings(&[], ClusterStop::Distance(0.05)).unwrap(),
        Vec::<usize>::new()
    );
    assert_eq!(
        cluster_embeddings(&[vec![1.0, 0.0]], ClusterStop::Distance(0.05)).unwrap(),
        vec![0]
    );
}

#[test]
fn s3_well_separated_embeddings_form_two_clusters() {
    // EP: two compact, well-separated speaker classes.
    let labels = cluster_embeddings(
        &[
            vec![1.0, 0.0],
            vec![0.995, 0.1],
            vec![0.0, 1.0],
            vec![0.1, 0.995],
        ],
        ClusterStop::Distance(0.05),
    )
    .unwrap();

    assert_eq!(labels, vec![0, 0, 1, 1]);
}

#[test]
fn s3_distance_threshold_includes_the_boundary_and_rejects_just_below_it() {
    // BVA: cosine distance is 0.04606; it merges at 0.05 but not at 0.04.
    let embeddings = [vec![1.0, 0.0], vec![0.9539392, 0.3]];

    assert_eq!(
        cluster_embeddings(&embeddings, ClusterStop::Distance(0.05)).unwrap(),
        vec![0, 0]
    );
    assert_eq!(
        cluster_embeddings(&embeddings, ClusterStop::Distance(0.04)).unwrap(),
        vec![0, 1]
    );
}

#[test]
fn s3_every_approved_sweep_threshold_has_a_bva_pair() {
    for threshold in [0.95, 0.90, 0.85, 0.80, 0.75] {
        // BVA: each ordered Route-A value merges a point just inside its
        // distance boundary and keeps a point just outside it separate.
        let just_inside = unit_pair_with_cosine_distance(threshold - 0.005);
        let just_outside = unit_pair_with_cosine_distance(threshold + 0.005);
        assert_eq!(
            cluster_embeddings(&just_inside, ClusterStop::Distance(threshold)).unwrap(),
            vec![0, 0],
            "threshold={threshold:.2} must include its inside boundary"
        );
        assert_eq!(
            cluster_embeddings(&just_outside, ClusterStop::Distance(threshold)).unwrap(),
            vec![0, 1],
            "threshold={threshold:.2} must reject its outside boundary"
        );
    }
}

fn unit_pair_with_cosine_distance(distance: f32) -> Vec<Vec<f32>> {
    let cosine = 1.0 - distance;
    vec![vec![1.0, 0.0], vec![cosine, (1.0 - cosine * cosine).sqrt()]]
}

#[test]
fn s3_invalid_embeddings_and_reserved_fixed_count_mode_are_reported() {
    // EP negative class: an all-zero vector has no cosine direction.
    assert!(cluster_embeddings(&[vec![0.0, 0.0]], ClusterStop::Distance(0.05)).is_err());
    // WP-49 owns fixed-speaker-count behavior; WP-62 reserves the interface.
    assert!(cluster_embeddings(&[vec![1.0, 0.0]], ClusterStop::FixedCount(2)).is_err());
}

#[test]
fn s1_pyannote_powerset_classes_expand_to_local_speaker_activity() {
    // Decision table: pyannote's no-speaker, singleton, and pair classes for
    // its three local speakers and powerset size two.
    assert_eq!(
        powerset_class_to_activity(0, 3, 2).unwrap(),
        vec![false, false, false]
    );
    assert_eq!(
        powerset_class_to_activity(1, 3, 2).unwrap(),
        vec![true, false, false]
    );
    assert_eq!(
        powerset_class_to_activity(3, 3, 2).unwrap(),
        vec![false, false, true]
    );
    assert_eq!(
        powerset_class_to_activity(4, 3, 2).unwrap(),
        vec![true, true, false]
    );
    assert_eq!(
        powerset_class_to_activity(6, 3, 2).unwrap(),
        vec![false, true, true]
    );
    assert!(powerset_class_to_activity(7, 3, 2).is_err());
}

#[test]
fn s1_segmentation_windows_keep_the_last_partial_audio_window() {
    // BVA: the final partial shift must be padded and retained, never dropped.
    assert_eq!(
        segmentation_windows(&[1.0, 2.0, 3.0, 4.0, 5.0], 4, 2).unwrap(),
        vec![(0, vec![1.0, 2.0, 3.0, 4.0]), (2, vec![3.0, 4.0, 5.0, 0.0])]
    );
    assert_eq!(
        segmentation_windows(&[1.0, 2.0, 3.0], 4, 2).unwrap(),
        vec![(0, vec![1.0, 2.0, 3.0, 0.0])]
    );
}
