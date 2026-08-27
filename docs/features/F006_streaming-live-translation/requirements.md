# F006 Streaming Live Translation & MFU Panel Toggle — Requirements

## Summary

Two additions to Streaming's transcript screen: a labelled **MFU panel switch**
present identically on both the Meeting and Streaming transcript headers
(view-only, per-screen persisted), and Streaming's own **Live Translation**
control — a header switch plus a locked English/Русский target-language
dropdown that, when on, splits the transcript into a paired-row original +
translation view. Translation runs locally through the same summary LLM
Craft MFU and Prettify already use (ADR-015), concurrently with live capture
on a single-flight backend queue, and persists per session/paragraph/target
language so a reopened session or a re-toggled switch reuses stored results
instead of re-running the model.

## Serves

- `idea.md` Value Proposition: "For live audio — a meeting in progress, your
  own dictated thoughts, or audio playing in the background — **Streaming**
  (ADR-014) gives a near-real-time, plain-text transcript you can copy or
  export as it happens, still entirely on-device."
- `idea.md` in-scope bullet: "**Streaming** (ADR-014): live, near-real-time
  transcription of microphone and/or system audio for a Streaming session …
  A separate, additive capability from Meeting."
- `idea.md` MFU-section scope bullet ("Structured **meeting MFU** generated
  locally … editable and copyable") for the panel-visibility requirement
  (F006-R1), which controls the same MFU surface on both screens.
- `idea.md` in-scope bullet: "**Streaming live translation** (ADR-015): a
  Streaming session's transcript can be translated into English or Russian as
  it is captured, shown beside the original, using the same local
  summarization model." Its Out-of-Scope entry was narrowed in the same pass
  so it now excludes only translation outside this pair and Meeting
  transcripts.
- [ADR-014](../../decisions/ADR-014-streaming-mode-coexists-with-batch-meeting.md)
  (Streaming's existence and offline scope) and
  [ADR-015](../../decisions/ADR-015-live-translation-shared-llm-concurrent-single-flight.md)
  (the translation-specific decisions this feature implements).

**Traceability gap — milestone only:** `roadmap.md` states that "Streaming's
own phase/milestone placement is not yet decided," so this feature — like the
rest of Streaming — has no milestone to trace up to. Every requirement traces
to an `idea.md` scope item and down to at least one task and one scenario;
only the milestone link is missing, and it is missing for all Streaming work,
not for this feature specifically. Reported for the coordinator rather than
invented here.

## Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| F006-R1 | The system shall show a labelled MFU panel switch at the right end of the transcript header action cluster on both the Meeting and Streaming screens, defaulting to on, persisted independently per screen across restarts, always enabled, and never gating or gated by Transcribe/Diarize/Craft MFU/Prettify/Live Translation. | must |
| F006-R2 | The system shall show a Live Translation control (label, switch, target-language dropdown offering English and Русский) in a middle slot of Streaming's transcript header, with the dropdown locked while the switch is on and the switch disabled with a stated reason when no LLM model is ready, a prettified transcript is active, or a Prettify review is pending. | must |
| F006-R3 | While Live Translation is on, the system shall render the transcript as a two-column paired-row grid (original left, translation right) inside the existing scroll container, one row per paragraph, reverting to the single-column view unchanged when switched off. | must |
| F006-R4 | The system shall translate one Streaming paragraph into the selected target language ("en"/"ru") through the same local llama.cpp/Qwen model used for summarization, validate the candidate, persist the result keyed by session/paragraph/target-language, and reuse a stored result instead of re-running the model unless the paragraph's source text has changed. | must |
| F006-R5 | The system shall run at most one translation call at a time (single-flight, independent of the live decode loop), enqueue a paragraph when it closes and backfill a session oldest-first when the switch turns on, and render each row's pending, translating, failed (with retry), or mirrored-original (same-language paragraph, no model call) state. | must |
| F006-R6 | The system shall include both the original and translated text, labelled and in screen order, in Streaming's copy and export output whenever translations exist for the selected target language, with an explicit placeholder for any paragraph that has no translation, and leave copy/export output unchanged when Live Translation is off. | must |

## Acceptance Criteria

- **F006-R1:** switching the panel off on Meeting stops `aside.wp-mfu` from
  rendering and the transcript takes the full workspace width; the same
  switch exists on Streaming; each screen's on/off state survives an app
  restart independently; running Craft MFU while the panel is hidden reveals
  it.
- **F006-R2:** the header shows label + switch + dropdown in a middle slot
  between the title group and the action cluster; the dropdown cannot be
  changed while the switch is on; the switch carries a stated disabled
  reason (not just a disabled state) in each of its three disabling
  conditions; Prettify is disabled with a stated reason while the switch is
  on, and vice versa.
- **F006-R3:** turning the switch on replaces the single-column transcript
  with a two-column grid where a paragraph and its translation share one
  grid row; turning it off restores the prior rendering unchanged; each
  column keeps a minimum width and the MFU aside keeps its fixed width at
  the app's minimum supported window width.
- **F006-R4:** `translate_streaming_paragraph` returns translated text and
  upserts one row in `streaming_translations`; a repeated call for the same
  `paragraph_key` updates the existing row rather than duplicating it; a row
  whose stored source text no longer matches the current paragraph is
  reported as stale by `list_streaming_translations`/`is_stale`.
- **F006-R5:** a live session's paragraphs enqueue as they close; turning
  the switch on backfills existing paragraphs oldest-first; only one
  translate call is ever in flight; a same-target-language paragraph shows
  the mirrored original with no model call; a failed paragraph shows a retry
  control without interrupting capture or the next paragraph's translation.
- **F006-R6:** exporting a session with complete translations produces one
  original+translation block per paragraph in screen order; a paragraph with
  no translation, a failed translation, or a mirrored paragraph is still
  emitted with an explicit not-translated placeholder so paragraph counts
  match on both sides; output with the switch off is byte-identical to the
  pre-existing single-language export.

## Constraints

- Translation reuses the bundled Qwen2.5 model via llama.cpp — no new model,
  no new model-catalog entry, offline only (ADR-006, ADR-014, ADR-015).
- Translation is single-flight via `AppState::translation_busy`, independent
  of `whisper_busy`; it shares the Metal GPU with live decoding and accepts
  that contention with transcription quality kept as the priority
  (ADR-015 Consequences).
- Craft MFU and Prettify's own stopped-only / mutual-exclusion rule is
  enforced only in the front end, not by a backend guard — unchanged by this
  feature (`docs/architecture.md` §Paragraph Translation; ADR-015).
- The paragraph (from `groupWindowsIntoParagraphs`), not the window or
  sentence, is the translation and alignment unit (ADR-015).
- Target languages are fixed at English and Russian; source language stays
  Streaming's existing per-window auto-detection (ADR-014) and is not
  reselected by this feature.
- The split paired-row view and the header's three-slot layout must remain
  usable at the app's minimum supported window width with the MFU panel
  open, including a narrow-width label-collapse behavior (`docs/design.md`
  §Center — transcript).
- Streaming and Meeting transcription remain mutually exclusive for v1
  (ADR-014); this feature does not change that.

## Out of Scope

- Translation on the Meeting screen — Meeting's header carries no
  translation control.
- Any target language other than English and Russian.
- Choosing or overriding the source language — it stays auto-detected per
  window (ADR-014); this feature only adds a target.
- A dedicated or downloadable machine-translation model — rejected in
  ADR-015; translation reuses the existing summarization model only.
- A backend concurrency guard for Craft MFU or Prettify — `translation_busy`
  covers only translation, per ADR-015.
- Retaining raw audio for later re-translation with a different model —
  Streaming retains no raw audio at all (ADR-014).
