# ADR-019: Capability-aware ASR with one shared Qwen3-ASR GGUF selection

- **Status:** accepted
- **Date:** 2026-09-12
- **Deciders:** Alexey Platkovsky
- **Relates to:** [ADR-003](ADR-003-whisper-turbo-metal.md),
  [ADR-012](ADR-012-auto-detect-only-transcription.md), and
  [ADR-017](ADR-017-recorder-retains-recoverable-local-audio.md)

## Context

Whisper was previously both the product model and an implicit architecture:
one file, one cached context, automatic language detection, timestamps, and all
application modes. ASR alternatives may have multi-file bundles and different
runtime, timestamp, streaming, memory, and quality boundaries. Selecting them
by catalog position or silently substituting Whisper would make a session's
provenance and behavior unknowable.

The Apple Silicon qualification initially included experimental pure-Rust
Qwen3-ASR 0.6B and 1.7B paths. The smaller model did not satisfy the product's
all-mode and mixed-language boundary. Maintaining a Recorder-only model also
required a second selection and language policy that contradicted the desired
single Transcription Models control. The official Qwen3-ASR 1.7B Q8_0 GGUF
conversion and `llama.cpp` MTMD path can serve bounded input in all three modes
without Python, MLX, or a sidecar process.

## Decision

WhisperPilot uses an explicit ASR model specification containing engine,
runtime, compatible modes, offline/streaming support, timestamp and
language-detection capabilities, memory guidance, license, and an exact asset
fingerprint. Resolution uses stable IDs and rejects unknown, incomplete, or
unsupported bundles before capture.

`active_model.transcription` is the only persisted ASR selection. It controls
future Meeting, local Streaming, and Recorder work. Existing installs without a
selection use Whisper, and persisted IDs no longer present in the catalog
normalize to Whisper. A selected engine is copied into each Recorder session so
historical provenance remains immutable.

Whisper remains the default. Qwen3-ASR 1.7B Q8_0 is the only optional Qwen ASR
and runs in-process through the existing `llama.cpp` backend with its MTMD audio
projector. Meeting uses app-controlled 30-second windows; Streaming and Recorder
use bounded live windows. The model protocol prefix becomes detected-language
metadata and is never shown as transcript text. Because the runtime does not
provide model timestamps, WhisperPilot assigns each result its capture or file
window span rather than presenting word timing.

The GGUF bundle pins ggml-org revision
`36a678687ba7d07a74ca70ccb0e36902e005fb80` and consists of
`Qwen3-ASR-1.7B-Q8_0.gguf` plus
`mmproj-Qwen3-ASR-1.7B-Q8_0.gguf`. Both files must pass catalog byte-size and
SHA-256 checks. Text LLM and GGUF-ASR models share one process-wide `llama.cpp`
backend while retaining separate model and inference-context caches.

The former Qwen3-ASR 0.6B catalog entry, pure-Rust `qwen-asr` runtime,
Recorder-specific model setting, and Recorder language setting are removed.
Its benchmark results remain historical evidence in the Phase 4 validation
report, not a shipped runtime or selectable model.

Fallback is explicit. A missing or unloadable selected bundle prevents Start
and points the user to model settings; it never silently starts Whisper under
the same session. ASR bundle deletion is blocked while Meeting transcription,
Streaming, or Recorder owns transcription resources. When idle, deleting the
selected Qwen bundle first resets the shared selection to Whisper and retains
that safe selection if a multi-file removal fails.

## Consequences

- The Transcription Models section has one radio per model and one choice for
  all three modes.
- Existing and removed-model settings deterministically return to Whisper
  without a migration prompt.
- Qwen-produced segment times are capture-window boundaries, not word or model
  timestamps, and the UI does not claim otherwise.
- A future ASR model must support every mode and pass the same corpus, memory,
  long-run, download, and real-device gates before entering the catalog.
- Qwen3-ASR 1.7B still needs full RU/EN/code-switch quality, latency, memory,
  and sustained-session qualification before comparative performance claims.

## Alternatives Considered

- **Keep 0.6B for Recorder only** — rejected because it fragments selection,
  language configuration, and runtime ownership without covering all modes.
- **Replace Whisper globally** — rejected because Whisper remains the proven
  default and the GGUF path has no native model timestamps.
- **Use an MLX or Python sidecar** — rejected because the existing in-process
  `llama.cpp` backend supports the selected GGUF plus its audio projector.
- **Silently fall back to Whisper** — rejected because it corrupts provenance
  and can change behavior inside what appears to be one selected session.
