# ADR-005: sherpa-onnx for speaker diarization

- **Status:** partially superseded (2026-07-26) — engine hosting only by ADR-013; the sherpa-onnx choice stands
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

"By roles" requires speaker diarization — determining who spoke when — which
Whisper cannot do alone. The engine must run fully on-device and integrate with a
Rust/Tauri app without a heavy runtime.

## Decision

Use **sherpa-onnx** speaker segmentation + embedding models (via its Rust
binding) for diarization in M2. It runs locally on ONNX with no Python, produces
speaker turns that are then merged onto Whisper segments.

## Consequences

- Fully local diarization consistent with the local-first principle; native Rust
  integration. *(Superseded in part by ADR-013: the engine call now runs in a
  child process, because its vendored C++ clustering can abort the host process
  outright. The binding is still native Rust with no runtime dependency.)*
- Requires downloading and verifying the segmentation and embedding models
  (mirrors the Whisper artifact-verification pattern).
- Quality is good but below pyannote's best; acceptable for generic,
  user-renamed speaker labels.

## Alternatives Considered

- **pyannote via a Python sidecar** — best-in-class diarization quality, but
  bundles a Python runtime and a process boundary; heavier and slower to ship.
- **whisper.cpp tinydiarize (tdrz)** — weak, limited to two-speaker turn hints.
