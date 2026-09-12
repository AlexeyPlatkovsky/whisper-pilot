# ADR-019: Capability-aware ASR and Qwen3-ASR for monolingual Recorder

- **Status:** accepted
- **Date:** 2026-09-12
- **Deciders:** Alexey Platkovsky
- **Relates to:** [ADR-003](ADR-003-whisper-turbo-metal.md),
  [ADR-012](ADR-012-auto-detect-only-transcription.md), and
  [ADR-017](ADR-017-recorder-retains-recoverable-local-audio.md)

## Context

Whisper was previously both the product model and an implicit architecture:
one file, one cached context, automatic language detection, timestamps, and all
application modes. Qwen3-ASR has a multi-file safetensors/tokenizer bundle and
different language, timestamp, streaming, memory, and quality boundaries.
Selecting it by catalog position or silently substituting Whisper would make a
recorded session's provenance and behavior unknowable.

The Apple Silicon qualification compared Whisper large-v3-turbo Q8 with
Qwen3-ASR 0.6B and 1.7B. Both Qwen variants were exact on the fixed monolingual
Russian and English corpus. The 0.6B model was faster and smaller. Neither
variant preserved mixed Russian/English speech reliably; the pure-Rust runtime
also exposes no model timestamps. The 1.7B model added no corpus accuracy while
roughly doubling memory and latency.

## Decision

WhisperPilot introduces an explicit ASR model specification containing engine,
compatible modes, offline/streaming support, timestamp and language-detection
capabilities, memory guidance, license, and an exact asset fingerprint. Model
selection resolves by stable ID and rejects unknown or unsupported combinations
before capture. The Qwen cache is keyed by engine, model ID, and the composite
fingerprint of its weights, vocabulary, and merges; Whisper retains its existing
single verified-model cache. The active engine is copied into each Recorder
session and cannot change for that session.

Whisper remains the default for Meeting, Streaming, mixed-language Recorder,
and any workflow requiring model timestamps or automatic language detection.
Qwen3-ASR 0.6B through the pinned pure-Rust `qwen-asr` 0.11.0 runtime is an
optional Recorder-only engine for explicitly selected Russian or English. It
decodes the same stable bounded, at-most-seven-second Recorder windows; the window pipeline owns
their session-relative spans because Qwen returns text without timestamps.

The downloadable Qwen bundle is the official revision
`5eb144179a02acc5e5ba31e748d22b0cf3e303b0`: `model.safetensors`, `vocab.json`,
and `merges.txt`. Every file has a pinned byte size and SHA-256 and the bundle
is ready only when every asset is present. There is no Python, CUDA, vLLM,
ONNX, or MLX production dependency. Apple Accelerate/vDSP is used by the
runtime.

Fallback is explicit. A missing, incomplete, incompatible, or unloadable Qwen
bundle prevents Start and points the user to the model/language setting; it
never starts Whisper under the same session. Deleting the inactive Qwen bundle
preserves existing sessions. Deleting the selected Qwen bundle is blocked while
Recorder is capturing or finalizing. When Recorder is idle, deletion first
resets the next selection to Whisper. It keeps that safe selection if any
multi-file removal fails because the bundle may then be partial. Existing
sessions retain their persisted engine and provenance.

## Consequences

- Existing installs deterministically retain Whisper without a redownload or
  settings migration prompt.
- Recorder users gain a much faster high-quality monolingual RU/EN option, but
  must select the language explicitly.
- Qwen-produced segment times are capture-window boundaries, not word or model
  timestamps, and the UI does not claim otherwise.
- A future ASR model or aligner must declare capabilities and pass the same
  corpus, memory, long-run, download, and real-device gates before promotion.
- Qwen3-ASR 1.7B and Qwen mixed-language mode remain rejected until evidence
  shows a material quality advantage and faithful code-switch preservation.

## Alternatives Considered

- **Replace Whisper globally** — rejected because Qwen does not satisfy mixed
  language, timestamps, Meeting, or current Streaming requirements.
- **Ship both Qwen sizes** — rejected because 1.7B consumed more memory and
  latency without improving accepted-corpus accuracy.
- **Use an MLX or Python sidecar** — rejected because the pure-Rust runtime met
  the adopted Recorder threshold and avoids a second deployment stack.
- **Silently fall back to Whisper** — rejected because it corrupts provenance
  and can change behavior inside what appears to be one selected session.
