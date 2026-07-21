# ADR-003: Whisper large-v3-turbo on Metal for transcription

- **Status:** accepted
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

The app needs an accurate, multilingual (Russian) speech-recognition model that
runs locally on Apple Silicon. A `large-v3-turbo` quantized artifact was already
managed on disk from VoicePilot, and whisper.cpp supports Metal GPU acceleration.

## Decision

Use Whisper **`large-v3-turbo` (q8_0)** via `whisper-rs` with the **`metal`**
feature and flash attention. For M1, reuse the existing on-disk artifact, with
the path overridable via `MFUPILOT_MODEL_PATH`.

## Consequences

- Strong Russian accuracy at high speed on the GPU (a 1-minute file transcribes
  in a few seconds).
- No model download needed for M1; the model is shared with VoicePilot's copy.
- WhisperPilot has no model catalog of its own yet — deferred (see roadmap
  non-goals). A future milestone adds managed model assets.

## Alternatives Considered

- **Full `large-v3` (non-turbo)** — marginally higher accuracy but slower; since
  turbo already reads well on real Russian audio, kept as a future lever, not the
  default.
- **`small`/`medium`** — insufficient accuracy for the product's core promise.
- **CPU-only build** — far slower; Metal is a straightforward, large win on the
  target hardware.
