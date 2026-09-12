# ADR-019: Capability-aware ASR with Qwen3-ASR native and GGUF runtimes

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
by catalog position or silently substituting Whisper would make a recorded
session's provenance and behavior unknowable.

The initial Apple Silicon qualification compared Whisper large-v3-turbo Q8
with Qwen3-ASR 0.6B and 1.7B in the pure-Rust runtime. Both Qwen variants were
exact on the fixed monolingual Russian and English corpus. The 0.6B model was
faster and smaller; neither preserved the fixed mixed-language sample. A later
product decision requested the official Q8_0 GGUF conversion and `llama.cpp`
MTMD path so the 1.7B model can be evaluated and used in all three modes
without introducing Python, MLX, or a sidecar process.

## Decision

WhisperPilot uses an explicit ASR model specification containing engine,
runtime, compatible modes, offline/streaming support, timestamp and
language-detection capabilities, memory guidance, license, and an exact asset
fingerprint. Selection resolves by stable ID and rejects unknown or unsupported
combinations before capture. Each Qwen cache is keyed by engine, model ID, and
the composite fingerprint of its complete asset bundle. The active engine is
copied into each Recorder session and cannot change for that session.

Whisper remains the default for every mode and for workflows requiring native
model timestamps.
Qwen3-ASR 0.6B through the pinned pure-Rust `qwen-asr` 0.11.0 runtime is an
optional Recorder-only engine for explicitly selected Russian or English. It
decodes the same stable bounded, at-most-seven-second Recorder windows; the window pipeline owns
their session-relative spans because Qwen returns text without timestamps.

Qwen3-ASR 1.7B Q8_0 runs in-process through the existing `llama.cpp` backend
and its MTMD audio projector. It is selectable for Meeting, Streaming, and
Recorder and accepts automatic language detection. Meeting uses app-controlled
30-second windows; Streaming and Recorder reuse the existing bounded live
windows. The model protocol prefix becomes detected-language metadata and is
never shown as transcript text. Since this runtime does not provide model
timestamps, WhisperPilot assigns each result its capture/file-window span
rather than presenting word timing.

The downloadable Qwen bundle is the official revision
`5eb144179a02acc5e5ba31e748d22b0cf3e303b0`: `model.safetensors`, `vocab.json`,
and `merges.txt`. Every file has a pinned byte size and SHA-256 and the bundle
is ready only when every asset is present. There is no Python, CUDA, vLLM,
ONNX, or MLX production dependency. Apple Accelerate/vDSP is used by the
runtime.

The 1.7B GGUF bundle pins ggml-org revision
`36a678687ba7d07a74ca70ccb0e36902e005fb80` and consists of both
`Qwen3-ASR-1.7B-Q8_0.gguf` and `mmproj-Qwen3-ASR-1.7B-Q8_0.gguf`. Both files
must pass their catalog byte-size and SHA-256 checks. The app shares one
process-wide `llama.cpp` backend between text LLM and GGUF-ASR models while
retaining separate model/context caches.

Fallback is explicit. A missing, incomplete, incompatible, or unloadable Qwen
bundle prevents Start and points the user to the model setting; it never starts
Whisper under the same session. Deleting any ASR bundle is blocked while
Meeting transcription, Streaming, or Recorder owns transcription resources.
When idle, deleting a selected Qwen bundle first resets each affected mode
selection to Whisper. It keeps that safe selection if any
multi-file removal fails because the bundle may then be partial. Existing
sessions retain their persisted engine and provenance.

## Consequences

- Existing installs deterministically retain Whisper without a redownload or
  settings migration prompt.
- Recorder retains the smaller monolingual 0.6B option; the 1.7B GGUF option
  can serve all three modes with automatic language detection.
- Qwen-produced segment times are capture-window boundaries, not word or model
  timestamps, and the UI does not claim otherwise.
- A future ASR model or aligner must declare capabilities and pass the same
  corpus, memory, long-run, download, and real-device gates before promotion.
- The 1.7B GGUF runtime still requires a real-Metal RU/EN/code-switch quality,
  latency, memory, and long-run qualification before comparative performance
  claims are made.

## Alternatives Considered

- **Replace Whisper globally** — rejected because Whisper remains the proven
  default and the GGUF path has no native model timestamps.
- **Use an MLX or Python sidecar** — rejected because the existing in-process
  `llama.cpp` backend supports the selected GGUF plus its audio projector.
- **Silently fall back to Whisper** — rejected because it corrupts provenance
  and can change behavior inside what appears to be one selected session.
