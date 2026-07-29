# ADR-005: sherpa-onnx for speaker diarization

- **Status:** partially superseded by ADR-013 (child-process isolation) and WP-62 (direct segmentation and Rust clustering; sherpa embedding extraction remains)
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

"By roles" requires speaker diarization — determining who spoke when — which
Whisper cannot do alone. The engine must run fully on-device and integrate with a
Rust/Tauri app without a heavy runtime.

## Decision

Use local pyannote segmentation and speaker-embedding models for diarization
in M2. WP-62 supersedes the original all-in-one sherpa-onnx execution choice:
the application runs the shipped segmentation ONNX model through Rust's `ort`
binding, post-processes powerset output and clusters embeddings in Rust, while
retaining sherpa-rs only for its public embedding-extractor boundary. It runs
locally with no Python and produces speaker turns that are then merged onto
Whisper segments.

## Consequences

- Fully local diarization consistent with the local-first principle. The direct
  path reuses the app's packaged ONNX Runtime dylib and avoids the vendored
  fast-clustering implementation. *(The native engine call remains isolated in
  a child process by ADR-013.)*
- Requires downloading and verifying the segmentation and embedding models
  (mirrors the Whisper artifact-verification pattern).
- Quality is good but below pyannote's best; acceptable for generic,
  user-renamed speaker labels.

## Alternatives Considered

- **pyannote via a Python sidecar** — best-in-class diarization quality, but
  bundles a Python runtime and a process boundary; heavier and slower to ship.
- **whisper.cpp tinydiarize (tdrz)** — weak, limited to two-speaker turn hints.
