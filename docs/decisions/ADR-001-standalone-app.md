# ADR-001: Standalone app, separate from VoicePilot

- **Status:** accepted
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

The offline file-transcription capability grew out of work on VoicePilot (a live
system-audio transcriber). The question was whether to add it as a mode inside
VoicePilot or build a separate application. The two share an engine (Whisper on
Metal) and model-management patterns, but differ sharply in UX (batch file
processing vs. a live tray app), lifecycle, and dependency weight (ffmpeg,
diarization, a local LLM).

## Decision

Build WhisperPilot as a **separate standalone Tauri application** in its own
repository, reusing VoicePilot's proven whisper+Metal integration and
model-management code as a starting point rather than re-deriving it.

## Consequences

- Clean separation: WhisperPilot's heavy new dependencies (ffmpeg, sherpa-onnx,
  llama.cpp) never bloat or destabilize VoicePilot's live path.
- The live app stays independently shippable while this one evolves.
- Cost: the Metal/whisper FFI build and model-download infra are maintained in
  two places. This was accepted; copying the working code neutralizes most of the
  duplication effort.

## Alternatives Considered

- **Mode inside VoicePilot** — maximized code reuse but coupled two very
  different UX/lifecycle products and risked the live app mid-tuning. Recommended
  by the assistant; the user chose separation for cleanliness.
- **Shared library, two thin apps** — more up-front structure than a small
  offline app warrants now; can be revisited if the shared surface grows.
