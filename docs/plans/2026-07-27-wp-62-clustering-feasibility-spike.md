# WP-62 Clustering Feasibility Spike

Date: 2026-07-27

## Question

Can WhisperPilot replace sherpa-onnx's vendored fast clustering with Rust-owned
threshold clustering while retaining local pyannote segmentation and the
existing child-process isolation contract?

## Evidence gathered

- The project uses `sherpa-rs` 0.6.8. Its public `Diarize` API constructs one
  all-in-one offline diarizer from segmentation, embedding, and clustering
  configuration, then returns only final `{ start, end, speaker }` results.
  It cannot expose segmentation output before the vendored clustering runs.
- The same binding separately exposes `speaker_id::EmbeddingExtractor`, which
  can calculate an embedding for supplied 16 kHz samples. That makes the
  embedding half available to a Rust-owned clustering path.
- The vendored sherpa-onnx C API exposes a standalone speaker-embedding
  extractor but exposes speaker segmentation only as a field of the all-in-one
  `OfflineSpeakerDiarization` configuration. There is no public C API to create
  a segmentation model, run it, or obtain its frames before clustering.
- The vendored C++ source does contain the pyannote segmentation model and its
  powerset-to-speaker-activity conversion. Therefore a focused fork could
  expose that boundary, but it would become a maintained fork and cannot be
  adopted without the required user approval.
- The configured pyannote segmentation archive is present at the WhisperPilot
  development support location and contains the expected `model.onnx` and its
  reference scripts. This confirms the model artifact is available for a later
  runtime experiment.
- The current `ort` release line is a Rust binding for ONNX Runtime and can
  load an ONNX Runtime dylib from an explicit path. Its current 2.0 release
  candidates require Rust 1.88, while WhisperPilot declares Rust 1.80. A route
  using `ort` therefore needs a compatible version selected and a bundled-dylib
  / code-signing plan before production adoption.
- The user supplied two known-two-speaker reference recordings: a 861.57-second
  recording at `/Users/Aleksei_Platkovskii/Documents/OBS/2026-07-24 11-30-17.mp4`
  and the 92.47-second prior crash-regression recording at
  `/Users/Aleksei_Platkovskii/Documents/OBS/2026-07-21 12-07-19.mp4`.
- Existing ignored regression coverage measured the 92.4-second recording as
  3 clusters using the crate defaults, but 1 cluster using the current tuned
  configuration. The tuned setting reduces over-clustering by merging the two
  real speakers, so it fails the known-speaker-count objective.
- The same coverage measured the 861.6-second recording as 22 clusters using
  the crate defaults and 7 clusters using the current tuned configuration. It
  completed without a native abort, but remains far above the known count of 2.

## Route assessment

| Route | Result | Reason |
| --- | --- | --- |
| A — direct ONNX Runtime from Rust | Approved | It is the only route that lets Rust own both the segmentation post-processing and clustering without a fork. The user approved this route; it still requires a compatible ONNX Runtime binding, bundling/code-signing work, and a runtime proof against the reference recordings. |
| B — focused sherpa-rs/sherpa-onnx fork | Technically viable, not approved | The required segmentation and powerset conversion already exist in vendored C++ but are below the public C API. Exposing them avoids a second runtime but creates fork-maintenance responsibility. |
| C — VAD segmentation | Feasible fallback, not selected | It would change segmentation semantics and lose pyannote overlap/turn precision; it cannot demonstrate the stated quality objective without comparison recordings. |

## Implemented result (2026-07-28)

Route A is implemented with `ort` v1.16.3 from its upstream git tag. It is
compatible with WhisperPilot's Rust 1.80 minimum and dynamically loads the
already-bundled ONNX Runtime 1.17.1 dylib. The runtime resolves that dylib from
the executable or app `Contents/Frameworks`, where WP-60's packaging pipeline
already stages and signs it; no additional native artifact is introduced.

The production path now runs the downloaded pyannote segmentation ONNX model
directly, expands its powerset classes into local-speaker activity, derives
speaker embeddings through the existing sherpa extractor, and assigns them
with Rust-owned deterministic incremental-centroid threshold clustering. The
old all-in-one `sherpa-rs::diarize::Diarize` path is not compiled into the
production route. ADR-013's child-worker containment remains in place because
the direct ONNX/native inference boundary is still external code.

The approved ordered sweep used both user-provided known-two-speaker videos.
The primary metric is cluster count; when there are more than two clusters,
the secondary metric is the duration share held by the two dominant clusters.

| Threshold / path | 92.47 s recording | 861.57 s recording | Interpretation |
| --- | --- | --- | --- |
| Vendored baseline (0.50) | 3 clusters; top two 98.46% | 22 clusters; top two 48.51% | over-clusters both recordings |
| Route A 0.95 | 1 cluster; top two 100.0% | 1 cluster; top two 100.0% | merges speakers |
| Route A 0.90 | 1 cluster; top two 100.0% | 1 cluster; top two 100.0% | merges speakers |
| Route A 0.85 | 1 cluster; top two 100.0% | 2 clusters; top two 100.0% | long recording matches known count |
| Route A 0.80 | 1 cluster; top two 100.0% | 2 clusters; top two 100.0% | long recording matches known count |
| Route A 0.75 | 1 cluster; top two 100.0% | 3 clusters; top two 85.70% | long recording over-clusters |

The corrected baseline-and-Route-A test completed in 795.74 seconds with no
native abort. The user selected `0.85` as the accepted automatic threshold:
it is within the long-recording two-cluster plateau and keeps distance from the
0.75 over-clustering edge. The short
recording merges at every approved point, so no single value in this sweep
meets the two-speaker objective for both recordings.

## Recommendation

Route A is complete as the implementation route: it keeps segmentation
post-processing and automatic threshold clustering in Rust without maintaining
a sherpa fork. The accepted runtime default is 0.85. Route B remains a fallback
only if a later direct-runtime issue requires it.

## Scope boundary

The original spike did not change production behavior. Its approved Route-A
implementation now changes the direct diarization runtime/dependency and
clustering path, but does not change the model catalog or remove ADR-013
process isolation.
