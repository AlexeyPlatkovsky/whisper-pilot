# F006 Streaming Live Translation & MFU Panel Toggle — Tasks

All tasks below are shipped; TaskPilot is authoritative for status (all listed
items are `done`). WP-91 is a standalone feature item (no parent epic); WP-92,
WP-93, and WP-94 are its children. WP-90 is a separate, unparented item whose
scope spans both the Meeting and Streaming screens.

| ID | Task | Implements | Depends on | TaskPilot |
| --- | --- | --- | --- | --- |
| F006-T1 | MFU panel switch on both the Meeting (`src/App.tsx`) and Streaming (`src/StreamingView.tsx`) transcript headers; two independent persisted settings keys; auto-reveal on Craft MFU while hidden | F006-R1 | — | WP-90 |
| F006-T2 | `llm::translate_paragraph` + `build_translate_prompt` + `validate_translation_candidate`; `AppState::translation_busy` / `TranslationUsageGuard` single-flight guard; `translate_streaming_paragraph` command; `streaming_translations` table (`streaming_store.rs`, upsert-on-conflict, cascade delete) | F006-R4, F006-R5 | — | WP-92 |
| F006-T3 | `list_streaming_translations` read command + `StreamingStore::list_translations` + `StreamingTranslation::is_stale`; `src/ipc.ts` typed wrappers (`translateStreamingParagraph`, `listStreamingTranslations`) for both commands | F006-R4 | F006-T2 | WP-93 |
| F006-T4 | Live Translation header control: label, switch, target-language select in the transcript header's middle slot; locked dropdown while on; disabled-with-reason states; Prettify/Live-Translation mutual exclusion | F006-R2 | F006-T2, F006-T3 | WP-93 |
| F006-T5 | Split paired-row transcript pane: two-column grid in the existing scroll container, one row per paragraph, column minimums, MFU-aside fixed width, narrow-width label collapse | F006-R3 | F006-T4 | WP-93 |
| F006-T6 | Translation queue: enqueue on paragraph close, oldest-first backfill on switch-on, single in-flight call, pending/translating/failed/mirrored row states, per-row retry, re-enqueue on paragraph boundary shift | F006-R5 | F006-T4, F006-T5 | WP-93 |
| F006-T7 | Paired copy/export rendering: labelled original+translation blocks in screen order, not-translated placeholder for missing/failed/stale/mirrored paragraphs, unchanged output when Live Translation is off | F006-R6 | F006-T6 | WP-94 |

## Notes

- Sequencing followed the TaskPilot link graph: WP-92 blocks WP-93, which
  blocks WP-94; WP-90 (the MFU toggle) shipped independently and is only
  linked to WP-93 via `relates_to` (Streaming's header layout needed a stable
  MFU-toggle slot before the Live Translation slot was added next to it).
- `list_streaming_translations` (F006-T3) is credited to WP-93 in
  `docs/architecture.md`, even though the read path lives in
  `commands/streaming.rs` next to the write path from WP-92 — WP-92 shipped
  persistence with no read-back, and WP-93 closed that gap. Recorded here to
  match the architecture doc rather than assume the file location implies the
  owning task.
- WP-91's own DoD requires a real-Metal verification run with mixed
  Russian/English audio confirming transcription is unaffected by concurrent
  translation; see `scenarios.md`'s manual verification checklist.
- This feature was never split into its own roadmap milestone; see
  `requirements.md` §Serves for the flagged `idea.md`/`roadmap.md`
  traceability gap.
