# ADR-016: Live translation moves from paragraph-unit to rolling per-window translation

- **Status:** accepted
- **Date:** 2026-08-28
- **Deciders:** Alexey Platkovsky
- **Relates to:** [ADR-015](ADR-015-live-translation-shared-llm-concurrent-single-flight.md)
  (engine reuse, single-flight concurrency, target languages, and the
  persistence-reuse concept — all still current); this ADR supersedes only
  ADR-015's "Unit: the paragraph is the translation and alignment unit"
  decision and its "Translate at window granularity" rejected alternative.

## Context

ADR-015 chose the paragraph (as grouped by `groupWindowsIntoParagraphs`) as
Live Translation's unit, explicitly rejecting window granularity as "a
decode-cadence artifact, not a stable linguistic unit." In practice this
produced up to `WINDOW_SECONDS * MAX_WINDOWS_PER_PARAGRAPH` (originally
42 seconds, 28 after WP-100 lowered the cap) of latency between speech and
its translation appearing on screen — far from the "at the same time as
transcription, or with a short delay" the user wanted after using the
feature live. WP-100 (lowering the window-count cap from 6 to 4, and adding
prior-paragraph context) narrowed that gap without changing the unit itself,
but a paragraph is still the wrong grain for near-real-time translation: it
only closes on a sentence-end heuristic or a window-count cap, both of which
can withhold translation for many seconds of already-decoded speech.

## Decision

**Unit: a single window (`WINDOW_SECONDS`, 7s), not a paragraph.** Nothing
translates until a session has at least 2 windows; at that point window 0
(no context) and window 1 (context = window 0's translation) both translate
immediately, back to back. Every window after that translates alone, as soon
as it arrives, with no wait for a paragraph to close — translation triggering
is now completely decoupled from `groupWindowsIntoParagraphs`, which
continues to exist solely to group the on-screen original-text display into
paragraphs.

**Context: a rolling window of the up-to-2 immediately preceding windows'
translations**, concatenated in order, replacing ADR-015/WP-100's "one prior
paragraph's translation." A failed or not-yet-available preceding window is
skipped rather than blocking — translation always proceeds, with less
context in that case, never a wait.

**Persistence: one row per window, not per paragraph.** The
`streaming_translations` table's `paragraph_key` column is renamed to
`window_index` (keyed by that window's own index, via a checked
`ALTER TABLE ... RENAME COLUMN` migration preserving existing rows); the
`translate_streaming_paragraph` command is renamed
`translate_streaming_window` to match. A window whose own language already
matches the target is still mirrored individually with no model call — now a
genuinely per-window check, so a paragraph mixing already-translated and
needs-translation windows renders correctly instead of ADR-015's
all-or-nothing paragraph-level check.

**Rendering stays paragraph-grouped.** The paired-row grid still shows one
row per paragraph (unchanged UX from ADR-015/WP-93) — only how its translated
cell is *filled* changes: it is built by mapping each of the paragraph's
windows through its own entry (real text for a done/mirrored window, a
placeholder for one still in flight or failed) and joining them, so a
paragraph with an in-progress trailing window shows real text for its
finished windows and a placeholder only for the unfinished tail, instead of
the whole cell staying blank until every window in it is done. The retry
affordance stays paragraph-scoped (one button, re-enqueuing every failed
window in that paragraph) rather than one button per window, to avoid a
cluttered multi-affordance row.

## Consequences

- **Translated text now appears roughly 7-14s behind speech**, replacing the
  up-to-28s (post-WP-100) / 42s (pre-WP-100) worst case — the latency
  complaint that motivated this decision is resolved by construction, not by
  further tuning a paragraph-sized cap.
- **A single 7s window frequently ends mid-sentence.** ADR-015 rejected
  window granularity specifically because a window is not a stable
  linguistic unit; this decision accepts that a window's own translation may
  read as a sentence fragment in isolation, leaning on rolling context to
  help the model continue naturally rather than on waiting for a complete
  sentence. This is a deliberate latency-over-per-call-coherence trade-off,
  not an oversight of ADR-015's original concern — the concern was correct
  and is knowingly accepted here because the feature's whole purpose (a
  session read *as it happens*) failed under the old latency.
- **Every session with translations persisted under the pre-ADR-016
  `paragraph_key` scheme retranslates once, harmlessly, the first time it is
  reopened after this ships** — the column rename changes what the
  persistence key means, so old keys cannot match new per-window lookups. A
  one-time, accepted migration cost, not a defect.
- ADR-015's engine choice (reuse the summary LLM), concurrency model
  (`translation_busy` single-flight, independent of `whisper_busy`), target
  languages (`en`/`ru` only), and the general principle of persisting to
  avoid re-running the model are all unchanged and still govern this
  feature — only the unit that gets persisted and sent to the model changed.

## Alternatives Considered

- **Keep the paragraph as the unit and further lower `MAX_WINDOWS_PER_PARAGRAPH`**
  — rejected: even a 2-window cap (14s) still withholds translation for an
  unbroken 14s stretch, and a very low cap starts fighting the sentence-end
  heuristic that exists to keep paragraphs linguistically coherent for
  *display* grouping — better to decouple the translation unit from the
  display-grouping heuristic entirely than to keep compressing one number.
- **Translate every window but keep the paired-row grid one-row-per-window
  too** (abandon paragraph-grouped display) — rejected: this would be a much
  larger, choppier UI change (many more, shorter rows) for no benefit over
  keeping the existing paragraph-grouped display and only changing how its
  translated cell fills in; the paragraph remains the right unit for what a
  reader perceives as one thought, even though it is no longer the right
  unit for triggering a model call.
- **Per-window retry affordance** (a button per failed window, not per
  paragraph) — rejected for this iteration: adds UI clutter for a case
  (multiple failed windows in one paragraph, needing independent retry) that
  is expected to be rare; a paragraph-level "retry every failed window here"
  button covers the common case with one control per row.
