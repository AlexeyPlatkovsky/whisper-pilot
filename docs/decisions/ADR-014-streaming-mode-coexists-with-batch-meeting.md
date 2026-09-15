# ADR-014: Meeting live capture coexists with Transcription's batch-accuracy pipeline

- **Status:** accepted
- **Date:** 2026-08-06
- **Deciders:** Alexey Platkovsky
- **Supersedes:** [ADR-002](ADR-002-offline-batch-transcription.md) on the
  app-wide scope boundary only (its full-file batch decision and accuracy
  rationale for Transcription still stand)

## Context

ADR-002 decided WhisperPilot would do full-file batch transcription, not live
streaming: VoicePilot's live pipeline was VAD-chopped, worked from only a short
rolling context window, and committed words under a real-time deadline, which
produced fragmented, lower-accuracy transcripts. WhisperPilot's purpose was
accuracy on recorded files, where no real-time constraint exists. That
reasoning is still correct for Transcription and does not change here.

Separately, a genuinely new need surfaced: transcribing live audio as it
happens — a meeting in progress, one's own dictated thoughts, background audio
such as a video playing — where a several-second lag is acceptable but waiting
for a recording to finish and then batch-process is not. This is not a request
to make Transcription real-time; it is a request for a second capability WhisperPilot
did not previously offer at all.

## Decision

Add **Meeting** as a second, architecturally separate capture mode alongside
Transcription. The implementation retains legacy `Streaming` identifiers:

- Meeting captures system audio only (loopback), excluding the microphone,
  and decodes on
  rolling ~5-10s windows using the same bundled `large-v3-turbo` Whisper model
  Transcription uses — no new model — re-detecting language per window so mixed-
  language input (e.g. English/Russian/Turkish within one session) is handled.
  This differs from Transcription's single per-file auto-detected language
  (ADR-012), because a live session has no fixed single language the way a
  finished file does.
- Meeting produces plain, unattributed running text — no speaker
  diarization/roles at all, unlike Transcription (ADR-005/ADR-013).
- Meeting is mutually exclusive with an active Transcription run for v1,
  since both share the one cached Whisper context in `AppState` — a resource-
  contention constraint, not a product choice.
- Meeting persists incrementally to a new, separate `streaming_sessions`
  store (not the `meetings` table). No raw audio is retained, only decoded
  text.
- Meeting's own priority order is quality/precision first, latency second:
  the 5-10s figure is a target ceiling, not something to optimize below at
  accuracy's expense — consistent with ADR-002's accuracy-first spirit for
  Transcription, even though the mechanism (rolling windows vs. full-file) differs.

This ADR marks ADR-002 as **partially superseded**: ADR-002's decision and
rationale stand unchanged for Transcription; only its implicit scope boundary —
"not live streaming" for the app as a whole — is superseded by Meeting's
addition.

## Consequences

- The app needs a system-audio-loopback (ScreenCaptureKit-class) macOS
  permission. It does not request microphone access for Meeting.
- Meeting and Transcription cannot run concurrently for v1 — a real
  UX constraint (starting one while the other is active is blocked with an
  explanatory message), not merely an implementation detail.
- A Meeting can never be re-transcribed later with a different model
  or setting, because no raw audio is retained by design.
- A feasibility spike measuring rolling-window decode latency for
  `large-v3-turbo` across supported Mac hardware is required before
  Meeting's own latency DoD can be finalized (precedent: WP-62's clustering
  feasibility spike).
- `docs/idea.md` and `docs/roadmap.md` no longer state that live capture is
  categorically out of scope; both now describe Meeting as an in-scope,
  additive capability alongside Transcription.

## Alternatives Considered

- **Reuse Transcription's full-file pipeline for Meeting too** — rejected; a
  full-file transcript does not exist until the source file is finalized,
  which defeats the near-real-time purpose entirely.
- **A smaller/faster dedicated streaming model** — considered and rejected in
  favor of reusing `large-v3-turbo`: no new model to catalog/download, and
  materially better accuracy on Russian/Turkish and mixed-language input,
  consistent with quality being prioritized over latency.
- **Fold Meeting into the legacy `meetings` table/entity** (a `source_kind` field
  on the existing table) — rejected. Meeting has no backing file, a
  possibly-mixed per-window language rather than one per-file language, and no
  diarization; none of these fit the existing Transcription-shaped fields cleanly, so
  a separate `streaming_sessions` entity was chosen instead.
