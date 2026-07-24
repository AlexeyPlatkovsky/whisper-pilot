# ADR-007: Russian-first, English added later

- **Status:** partially superseded (2026-07-23) — the language-selection decision
  below is replaced by ADR-012; the Russian-first product focus still stands
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

The immediate need is accurate transcription of Russian meeting audio, which
cloud English-first tools handle poorly. The chosen model (`large-v3-turbo`) is
multilingual, so both languages are technically supported; the question is
product focus and default.

## Decision

Make **Russian the primary language and default** (`language = "ru"`, UI copy in
Russian), with an explicit per-transcription **auto-detect** option that lets
Whisper choose. Add first-class English as a follow-on after M1–M3 are stable —
a small change that does not alter the pipeline shape.

> **Superseded by [ADR-012](ADR-012-auto-detect-only-transcription.md)
> (2026-07-23)** on the language mechanism only. Whisper now always detects the
> language and the user cannot select one; `language = "ru"` as a forced decode
> default caused a real defect (WP-20). The Russian-first *product focus* — what
> the app is optimized and validated for, and the ordering of the English
> follow-on — is unchanged.

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
- **Auto-detect as the only mode** — convenient but misdetects on short/quiet
  clips; offered as an option alongside the Russian default rather than as the
  default itself. *(This is the alternative ADR-012 later adopted: forcing a
  default proved far more damaging in practice than a misdetection, because a
  wrong forced language does not degrade the transcript — it replaces it.)*
