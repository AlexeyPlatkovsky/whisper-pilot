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
| `architecture.md` | Layer map, pipeline, IPC contract, models, build, ownership | You need technical structure, components, or constraints |
| `design.md` | Product/UX design: flows, screens, states, interaction, accessibility | You need UX flows or view states |
| `testing.md` | Test strategy, levels, running scenarios, coverage, quality gates | You need how quality is verified |
| `roadmap.md` | Phases (M1–M3), sequencing, non-goals over time | You need the release plan or priorities |
| `glossary.md` | Domain vocabulary (diarization, turn, segment, MFU, …) | You hit an unfamiliar term |
| `decisions/` | One ADR per significant decision (rationale) | You need why a choice was made |

## Feature Registry

| ID | Feature | Requirements | Tasks | Scenarios | Serves | TaskPilot |
| --- | --- | --- | --- | --- | --- | --- |
| F001 | File transcription | R1–R6 | T1–T6 | S1–S5 | idea: transcribe files · roadmap: M1 | — (as-built) |
| F002 | Speaker diarization | R1–R5 | T1–T6 | S1–S4 | idea: speaker-attributed · roadmap: M2 | epic WP-1 (WP-5…WP-10) |
| F003 | Summary / MFU | R1–R5 | T1–T3 | S1–S3 | idea: local summary · roadmap: M3 | — (unscheduled) |

## Decision Log

| ADR | Title | Status |
| --- | --- | --- |
| ADR-001 | Standalone app, separate from VoicePilot | accepted |
| ADR-002 | Offline full-file (batch) transcription, not live streaming | accepted |
| ADR-003 | Whisper large-v3-turbo on Metal for transcription | accepted |
| ADR-004 | ffmpeg as the single audio/video ingestion path | accepted |
| ADR-005 | sherpa-onnx for speaker diarization | accepted |
| ADR-006 | llama.cpp + Qwen2.5 for local summarization | accepted |
| ADR-007 | Russian-first, English added later | accepted |

## Traceability Notes

- Every F-requirement traces up to an `idea.md` scope item and a `roadmap.md`
  phase, and down to at least one task and one scenario (see each feature folder).
- Work status lives in TaskPilot (`WP-<n>`); F002 tasks carry their WP IDs. F001
  is as-built (pre-tracking); F003 is unscheduled until M3 is picked up.
