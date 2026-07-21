# Documentation Index

Live map of the docs tree, feature registry, and decision log. Lookup aid only —
no routing, gates, or behavioral rules. Rebuilt by `sdd-index-sync` after any
doc/feature change.

**Tier:** Standard
**Docs root:** `docs/`

## Documents

| Document | Owns | Read when |
| --- | --- | --- |
| `idea.md` | Problem, users, value, scope, non-goals, principles, success signals | You need project intent or scope boundaries |
| `architecture.md` | Layer map, pipeline, meeting/data model, IPC contract, models, build | You need technical structure or constraints |
| `design.md` | Product/UX design: two-pane shell, header, status bar, transcript, MFU section, Settings, states | You need UX flows or view states |
| `testing.md` | Test strategy, levels, running scenarios, coverage, quality gates | You need how quality is verified |
| `roadmap.md` | Milestones M1–M3, sequencing, non-goals over time | You need the release plan or priorities |
| `glossary.md` | Domain vocabulary (meeting, library, MFU section, diarization, …) | You hit an unfamiliar term |
| `decisions/` | One ADR per significant decision (rationale) | You need why a choice was made |

## Feature Registry

Listed in milestone order (feature IDs are stable, not sequential with
milestones).

| ID | Feature | Milestone | Requirements | Tasks | Scenarios | TaskPilot |
| --- | --- | --- | --- | --- | --- | --- |
| F001 | Transcription core | M1 (done) | R1–R6 | T1–T6 | S1–S5 | — (as-built) |
| F004 | Library & workspace | M2 (next) | R1–R14 | T1–T15 | S1–S14 | epic WP-11 (WP-16…WP-30, WP-32) |
| F002 | Speaker diarization | M2 (next) | R1–R7 | T1–T7 | S1–S6 | epic WP-1 (WP-5…WP-10, WP-31) |
| F005 | Settings | M2 beta / M3 release | R1–R10 | T1–T10 | S1–S10 | epic WP-33 (WP-34…WP-42); release unscheduled |
| F003 | Structured meeting notes | M3 | R1–R6 | T1–T4 | S1–S4 | — (unscheduled) |

## Decision Log

| ADR | Title | Status |
| --- | --- | --- |
| ADR-001 | Standalone app, separate from VoicePilot | accepted |
| ADR-002 | Offline full-file (batch) transcription, not live streaming | accepted |
| ADR-003 | Whisper large-v3-turbo on Metal for transcription | accepted |
| ADR-004 | ffmpeg as the single audio/video ingestion path | accepted |
| ADR-005 | sherpa-onnx for speaker diarization | accepted |
| ADR-006 | llama.cpp + Qwen2.5 for local summarization | accepted |
| ADR-007 | Russian-first, English added later (auto-detect option) | accepted |
| ADR-008 | Persisted meeting library (SQLite), reference-only audio, auto-save | accepted |
| ADR-009 | Structured meeting notes (full set), editable | accepted |
| ADR-010 | Two-pane shell; manual Transcribe/MFU triggers; colored bubbles from M2 | accepted |
| ADR-011 | Settings: in-app model management, theming, i18n, English-default UI | accepted |

## Traceability Notes

- Every F-requirement traces up to an `idea.md` scope item and a `roadmap.md`
  milestone, and down to at least one task and one scenario (see each feature
  folder).
- Work status lives in TaskPilot (`WP-<n>`). Active backlog: **M2 (Beta)** =
  **F004 Library & workspace** (epic WP-11, features WP-12…WP-15 + WP-25, tasks
  WP-16…WP-30 + WP-32), **F002 Speaker diarization** (epic WP-1, features
  WP-2…WP-4, tasks WP-5…WP-10 + WP-31), and **F005 Settings — beta** (epic WP-33,
  features WP-34…WP-36, tasks WP-37…WP-42). F001 is as-built (pre-tracking);
  **M3 (Release)** = **F003 notes** and the **F005 release scope**, both
  unscheduled in TaskPilot until M3 is picked up.
