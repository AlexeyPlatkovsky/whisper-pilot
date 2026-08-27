# ADR-015: Live translation reuses the summary LLM and runs concurrently on a single-flight queue

- **Status:** accepted
- **Date:** 2026-08-27
- **Deciders:** Alexey Platkovsky
- **Relates to:** [ADR-014](ADR-014-streaming-mode-coexists-with-batch-meeting.md)
  (Streaming's scope and offline stance), [ADR-006](ADR-006-llamacpp-qwen-summary.md)
  (the llama.cpp + Qwen2.5 summarization stack this decision reuses)

## Context

Streaming (ADR-014) needed a way to read a live session in a language the user
does not speak, without waiting for the session to end. WhisperPilot already
runs a local LLM — llama.cpp + a quantized Qwen2.5-Instruct model — for
Meeting's structured MFU (ADR-006) and, more recently, for Streaming's own
Prettify rewrite. A decision was needed on what model powers translation, and
on when translation is allowed to run relative to live capture.

Two prior features already answer the "when" question one way: Craft MFU
(Meeting) and Prettify (Streaming) are both gated, in the front end, to run
only while their session is **stopped** — never concurrently with active
transcription — because both share the one cached Whisper context in
`AppState` and the project has so far treated LLM work and live decoding as
mutually exclusive to protect transcription quality. Live translation's own
purpose — reading a session *as it happens* — cannot accept that constraint;
a translation feature that only worked after the user stopped capturing would
defeat the point of "live."

## Decision

**Engine: reuse the bundled summary LLM, add no translation model.**
`llm::translate_paragraph` calls the same llama.cpp completion path Craft MFU
and Prettify already use, against whatever Qwen2.5-Instruct model is
currently active — no new model asset, no new Settings → AI models catalog
entry, no new download. Translation is offline like every other on-device
capability (ADR-006, ADR-014). It runs its own prompt
(`build_translate_prompt`) and its own candidate validation
(`validate_translation_candidate`), in the same spirit as
`build_prettify_prompt` / `validate_prettify_candidate`, but reusing the model
means translation quality is bounded by a small general-purpose model, not a
model chosen or tuned for machine translation.

**Concurrency: translation runs concurrently with live capture, on a
single-flight queue — a deliberate departure from the Craft/Prettify
stopped-only rule.** Unlike Craft MFU and Prettify, live translation is only
useful while a session is running, so it is allowed to call the LLM while
Whisper is actively decoding rolling windows on the same GPU. To keep this
safe:

- `AppState::translation_busy` (a new `AtomicBool`) is a single-flight guard
  that admits at most one in-flight translation call at a time, claimed and
  released via `llm::TranslationUsageGuard`, an RAII guard mirroring
  `streaming_session::WhisperUsageGuard`'s claim/release idiom. A second
  concurrent request returns `AppError::TranslationBusy`, a distinct,
  UI-retryable error, rather than queuing indefinitely or blocking the caller.
- `translation_busy` is deliberately independent of `whisper_busy`: it never
  blocks, and is never blocked by, the streaming decode loop. Translation and
  transcription contend for the same Metal GPU, but neither is allowed to
  hold a lock the other needs — decode keeps its rolling-window cadence
  regardless of translation activity.
- This is the **first real backend concurrency guard** the project has added
  for LLM work. Craft and Prettify's mutual exclusion (stopped-only, and
  exclusion from each other) is still enforced **only in the front end** —
  nothing in the core itself stops two concurrent Craft/Prettify calls from
  both reaching the shared model. That gap is unchanged by this decision;
  translation does not retroactively guard Craft or Prettify, and
  `translation_busy` does not cover them either.

**Unit: the paragraph is the translation and alignment unit.** A paragraph
(as grouped by the front end's `groupWindowsIntoParagraphs`) is translated and
persisted as one piece — not a window, not a sentence — because it is the
unit the split paired-row view aligns original and translated text on: one
paragraph, one row, on both sides.

**Target languages: English and Russian only.** The target-language control
offers exactly `en` and `ru`; source language keeps Streaming's existing
per-window auto-detection (ADR-014) — translation adds a target, it does not
change how the source is detected. A paragraph whose text is already
entirely in the target language is never sent to the model; its row mirrors
the original text instead, saving a model call for the common case of an
already-matching-language paragraph.

**Persistence: reuse translations instead of re-running the model.**
Translations persist keyed by `(session_id, paragraph_key, target_language)`
in the new `streaming_translations` table, alongside the source text they
were produced from. Turning Live Translation on backfills a session from
stored rows first; a row is only re-translated when its stored source text no
longer matches the paragraph's current text (the paragraph grew because a
later window was appended) — i.e., a row is stale precisely when its source
has changed, not on every read.

## Consequences

- **GPU contention with live Whisper decoding is accepted, with transcription
  quality kept as the priority.** Translation shares the same Metal GPU as
  the rolling-window decode loop that produces the live transcript. This
  decision accepts that contention rather than serializing translation behind
  session-stop, on the reasoning that a single-flight guard bounds
  translation to one concurrent call and the decode loop is never blocked
  waiting on it — but a live translation session is expected to compete for
  GPU time in a way a stopped-session Prettify/Craft call never did.
- **Translation quality is bounded by a small quantized general-purpose LLM**,
  not a model selected or tuned for machine translation. This is the same
  trade-off ADR-006 already accepted for summarization — local-first over
  best-achievable quality — extended to a second use of the same model.
- **The pre-existing Craft/Prettify stopped-only and mutual-exclusion rules
  remain front-end-only**, an inconsistency this decision does not resolve.
  A future change that wants the same guarantee for Craft or Prettify needs
  its own backend guard; `translation_busy` is scoped to translation alone.
- **The app's minimum supported window width now has to fit a two-column
  paired-row layout** (original + translation) alongside the MFU panel, on
  top of the single-column widths every other screen state already had to
  fit. This constrains how narrow the app's minimum width can go without the
  split view degrading, and shaped the header's narrow-width label-collapse
  behavior (see `docs/design.md`).
- Live Translation and Prettify remain mutually exclusive (each disabled with
  a stated reason while the other is active) — this is a front-end UX
  constraint carried over from Prettify's existing design, not a new backend
  rule; `translation_busy` does not enforce it.

## Alternatives Considered

- **Add a dedicated translation model to the catalog** (e.g. a compact
  NLLB/MADLAD-class MT model) — rejected: a new multi-GB downloadable asset,
  a new Settings → AI models entry, and new engine-hosting work, for a
  capability the bundled Qwen2.5 model can already perform adequately inside
  the existing llama.cpp path. Rejected for the same local-first,
  minimal-footprint reasoning ADR-006 applied to summarization.
- **Gate translation to stopped sessions, matching Craft/Prettify** —
  rejected: it would make "live" translation available only after the
  session ends, defeating the feature's purpose. Live translation is only
  valuable while capture is active.
- **Block or queue a second in-flight translation request indefinitely**
  (instead of single-flight rejection) — rejected: an unbounded queue could
  let translation work pile up faster than the model can drain it during a
  long session; returning a distinct, UI-retryable `TranslationBusy` error
  keeps the queue's pacing decision in the front end, where the existing
  one-in-flight backfill/live queue (WP-93) already needs it.
- **Translate at window granularity instead of paragraph granularity** —
  rejected: a window is a decode-cadence artifact (~5-10s), not a stable
  linguistic unit; the paragraph is what the split view aligns rows on and
  what a reader perceives as one translatable thought.
- **Support more target languages than English and Russian** — deferred, not
  rejected outright: the two languages match the app's current primary
  audiences (ADR-007's Russian-first stance, English added later); widening
  the target set is a future increment, not part of this decision.
