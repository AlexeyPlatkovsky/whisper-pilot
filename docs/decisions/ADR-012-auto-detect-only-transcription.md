# ADR-012: Transcription language is always auto-detected, never chosen

- **Status:** accepted
- **Date:** 2026-07-23
- **Deciders:** Alexey Platkovsky
- **Supersedes:** [ADR-007](ADR-007-russian-first.md) on the language mechanism
  (its Russian-first product focus still stands)

## Context

ADR-007 made Russian the forced decode default (`language = "ru"`) with an
opt-in per-transcription auto-detect toggle, and explicitly rejected auto-detect
as the only mode on the grounds that detection misfires on short or quiet clips.

Shipping that default exposed the asymmetry in that trade-off. A five-minute
English standup recording transcribed as **eleven identical lines of
`Субтитры сделал DimaTorzok`** — a Russian YouTube subtitle-credit artifact from
Whisper's training data — one per fixed 30-second analysis window, with the real
speech nowhere in the output (WP-20). Forcing a language Whisper's audio is not
in does not degrade the transcript; the decoder collapses onto the highest-prior
artifact for that language and **replaces** it. Audio ingestion and every decode
threshold were ruled out with evidence; changing only the language to auto
yielded `en` at p = 0.997 and a correct transcript.

The selector that would have let a user work around this was never built, so in
practice every meeting was forced to Russian with no way out.

## Decision

**Whisper always detects the language, and the user cannot choose one.** There
is no selector, no setting, and no default to override — `transcribe()` takes no
language argument at all, so the guarantee is structural rather than a
convention.

The language Whisper actually decoded with is read back from decoder state and
stored on the meeting. `meetings.language` is therefore an **output** of a
transcription, never an input to one.

## Consequences

- A wrong language can no longer silently replace a transcript; the worst case
  is a misdetection on genuinely ambiguous audio, which is visible in the output
  rather than disguised as fluent text.
- No data migration was needed for rows still holding the old `"ru"`: because
  nothing reads the column as an input, the next run overwrites it.
- Whisper detects from the **first 30 seconds** only, so a recording that opens
  with silence or music can misdetect. Accepted for now; detecting on a voiced
  window instead is the known remedy if it proves a problem in practice.
- The planned language-selection UI (F004-R6, F004-T5) is withdrawn rather than
  built.
- Russian-first *positioning* is unaffected: the app is still optimized and
  validated for Russian meeting audio, which the multilingual `large-v3-turbo`
  model detects and transcribes without being told.

## Alternatives Considered

- **Keep a default, add the selector** — the original ADR-007 plan. Rejected:
  it preserves a state where the app confidently emits a fabricated transcript,
  and puts the burden of noticing on the user.
- **Auto-detect by default with an optional override** — safer than ADR-007 but
  still ships a control whose only use is to reintroduce the failure mode; no
  user need for it was identified.
- **Detect on a voiced window rather than the first 30 seconds** — more robust,
  but a larger change to the audio path; deferred as the remedy if first-30s
  detection proves insufficient.
