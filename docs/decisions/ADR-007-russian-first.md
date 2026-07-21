# ADR-007: Russian-first, English added later

- **Status:** accepted
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

The immediate need is accurate transcription of Russian meeting audio, which
cloud English-first tools handle poorly. The chosen model (`large-v3-turbo`) is
multilingual, so both languages are technically supported; the question is
product focus and default.

## Decision

Make **Russian the primary language and default** (`language = "ru"`, UI copy in
Russian). Add English as a follow-on after M1–M3 are stable, via
language/model selection — a small change that does not alter the pipeline shape.

## Consequences

- The product is optimized and validated for Russian first; accuracy claims are
  made against Russian audio.
- English is low-risk to add later (the model already supports it); deferring it
  keeps early milestones focused.
- UI is Russian by default; an English/localization toggle is part of the
  English follow-on.

## Alternatives Considered

- **Bilingual from day one** — dilutes validation focus for no near-term benefit;
  the model supports it whenever needed.
- **Auto-detect language** — convenient later, but an explicit default is more
  predictable for the primary use case and avoids misdetection on short clips.
