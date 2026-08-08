# ADR-014: Streaming coexists with Meeting's batch-accuracy pipeline

- **Status:** accepted
- **Date:** 2026-08-06
- **Deciders:** Alexey Platkovsky
- **Supersedes:** [ADR-002](ADR-002-offline-batch-transcription.md) on the
  app-wide scope boundary only (its full-file batch decision and accuracy
  rationale for Meeting still stand)

## Context

ADR-002 decided WhisperPilot would do full-file batch transcription, not live
streaming: VoicePilot's live pipeline was VAD-chopped, worked from only a short
rolling context window, and committed words under a real-time deadline, which
produced fragmented, lower-accuracy transcripts. WhisperPilot's purpose was
accuracy on recorded files, where no real-time constraint exists. That
reasoning is still correct for Meeting and does not change here.

Separately, a genuinely new need surfaced: transcribing live audio as it
happens — a meeting in progress, one's own dictated thoughts, background audio
such as a video playing — where a several-second lag is acceptable but waiting
for a recording to finish and then batch-process is not. This is not a request
to make Meeting real-time; it is a request for a second capability WhisperPilot
did not previously offer at all.

## Decision

Add **Streaming** as a second, architecturally separate capture mode alongside
Meeting:

- Streaming captures mixed microphone + system-audio (loopback) and decodes on
  rolling ~5-10s windows using the same bundled `large-v3-turbo` Whisper model
  Meeting uses — no new model — re-detecting language per window so mixed-
  language input (e.g. English/Russian/Turkish within one session) is handled.
  This differs from Meeting's single per-file auto-detected language
  (ADR-012), because a live session has no fixed single language the way a
  finished file does.
- Streaming produces plain, unattributed running text — no speaker
  diarization/roles at all, unlike Meeting (ADR-005/ADR-013).
- Streaming is mutually exclusive with an active Meeting transcription for v1,
  since both share the one cached Whisper context in `AppState` — a resource-
  contention constraint, not a product choice.
- Streaming persists incrementally to a new, separate `streaming_sessions`
  store (not the `meetings` table). No raw audio is retained, only decoded
  text.
- Streaming's own priority order is quality/precision first, latency second:
  the 5-10s figure is a target ceiling, not something to optimize below at
  accuracy's expense — consistent with ADR-002's accuracy-first spirit for
  Meeting, even though the mechanism (rolling windows vs. full-file) differs.

This ADR marks ADR-002 as **partially superseded**: ADR-002's decision and
rationale stand unchanged for Meeting; only its implicit scope boundary — "not
live streaming" for the app as a whole — is superseded by Streaming's
addition.

## Consequences

- The app needs new macOS permissions it has never required before:
  microphone access and a system-audio-loopback (ScreenCaptureKit-class)
  permission.
- Streaming and Meeting transcription cannot run concurrently for v1 — a real
  UX constraint (starting one while the other is active is blocked with an
  explanatory message), not merely an implementation detail.
- A Streaming session can never be re-transcribed later with a different model
  or setting, because no raw audio is retained by design.
- A feasibility spike measuring rolling-window decode latency for
  `large-v3-turbo` across supported Mac hardware is required before
  Streaming's own latency DoD can be finalized (precedent: WP-62's clustering
  feasibility spike).
- `docs/idea.md` and `docs/roadmap.md` no longer state that live capture is
  categorically out of scope; both now describe Streaming as an in-scope,
  additive capability alongside Meeting.

## Alternatives Considered

- **Reuse Meeting's full-file pipeline for Streaming too** — rejected; a
  full-file transcript does not exist until the source file is finalized,
  which defeats the near-real-time purpose entirely.
- **A smaller/faster dedicated streaming model** — considered and rejected in
  favor of reusing `large-v3-turbo`: no new model to catalog/download, and
  materially better accuracy on Russian/Turkish and mixed-language input,
  consistent with quality being prioritized over latency.
- **Fold Streaming into the `meetings` table/entity** (a `source_kind` field
  on the existing table) — rejected. Streaming has no backing file, a
  possibly-mixed per-window language rather than one per-file language, and no
  diarization; none of these fit the existing Meeting-shaped fields cleanly, so
  a separate `streaming_sessions` entity was chosen instead.
