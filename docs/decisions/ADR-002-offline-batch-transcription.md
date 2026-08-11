# ADR-002: Offline full-file (batch) transcription, not live streaming

- **Status:** partially superseded (2026-08-06) — the app-wide "not live
  streaming" scope boundary is replaced by ADR-014; the full-file batch
  decision and its accuracy rationale for Meeting still stand
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

VoicePilot's live pipeline produced fragmented, lower-accuracy transcripts:
VAD chopped speech mid-sentence, only a short rolling window of context was
available, and words were committed under a real-time deadline. WhisperPilot's
purpose is accuracy and precision on recorded files, where no real-time
constraint exists.

## Decision

Transcribe the **entire file in one pass** with no VAD, no streaming, and no
provisional/commit cycle. Use beam search and the model's own fallbacks, giving
Whisper the full file as context.

> **Partially superseded by [ADR-014](ADR-014-streaming-mode-coexists-with-batch-meeting.md)
> (2026-08-06)** on the app-wide scope boundary only. WhisperPilot now also
> offers **Streaming**, a separate near-real-time capture mode for live audio.
> This decision's actual content — full-file batch decoding as *Meeting's*
> mechanism, chosen for its accuracy — is unchanged; only the earlier implicit
> claim that the app as a whole would never do anything live is superseded.

## Consequences

- Materially higher accuracy and well-formed sentences — confirmed in practice:
  the same class of audio that read poorly live reads cleanly offline.
- Simpler core than the live pipeline (no VAD, backlog, or overload handling).
- A file takes minutes to process with no partial output; the UI must show a
  clear in-progress state and, later, real progress. Accepted trade-off.

## Alternatives Considered

- **Reuse the live streaming pipeline** — carried exactly the fragmentation and
  context-loss problems this product exists to avoid.
- **Chunked pseudo-streaming for progress feedback** — unnecessary complexity;
  full-file decoding plus an indeterminate spinner and timer covers the current
  UX need without changing Whisper's Meeting callback configuration.
