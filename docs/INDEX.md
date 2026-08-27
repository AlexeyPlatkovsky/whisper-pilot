# Documentation Index

Live map of the docs tree and decision log. Lookup aid only — no routing,
gates, or behavioral rules. Rebuilt by `sdd-index-sync` after any doc change.

The two tables below sit inside generated `begin`/`end` marker comments. The
sync owns which rows exist — keyed by filename and ADR ID — and re-derives the
key cell. Every other cell is hand-written and carried across a rebuild
byte-identically, and text outside the markers is never touched. Full
contract: `.claude/skills/sdd-index-sync/SKILL.md`.

**Tier:** Standard
**Docs root:** `docs/`

## Documents

<!-- sdd-index-sync:begin documents -->
| Document | Owns | Read when |
| --- | --- | --- |
| `idea.md` | Problem, users, value, scope, non-goals, principles, success signals | You need project intent or scope boundaries |
| `architecture.md` | Layer map, pipeline, meeting/data model, IPC contract, models, build | You need technical structure or constraints |
| `design.md` | Product/UX design: two-pane shell, header, status bar, transcript, MFU section, Settings, states | You need UX flows or view states |
| `testing.md` | Test strategy, levels, running scenarios, coverage, quality gates | You need how quality is verified |
| `roadmap.md` | Milestones M1–M3, sequencing, non-goals over time | You need the release plan or priorities |
| `glossary.md` | Domain vocabulary (meeting, library, MFU section, diarization, …) | You hit an unfamiliar term |
| `development.md` | Developer guide: prerequisites, build/dev/test commands, project layout | You need to build or run WhisperPilot from source |
| `designbook.md` | UI design contract: design tokens (source of truth `src/tokens.css`), themes, component patterns | You do UI work and need tokens or patterns |
| `decisions/` | One ADR per significant decision (rationale) | You need why a choice was made |
<!-- sdd-index-sync:end documents -->

## Decision Log

<!-- sdd-index-sync:begin decisions -->
| ADR | Title | Status |
| --- | --- | --- |
| ADR-001 | Standalone app, separate from VoicePilot | accepted |
| ADR-002 | Offline full-file (batch) transcription, not live streaming | partially superseded by ADR-014 (app-wide scope boundary only; the full-file batch decision and its accuracy rationale for Meeting stand) |
| ADR-003 | Whisper large-v3-turbo on Metal for transcription | accepted |
| ADR-004 | ffmpeg as the single audio/video ingestion path | accepted |
| ADR-005 | sherpa-onnx for speaker diarization | partially superseded by ADR-013 (engine hosting only; sherpa-onnx choice stands) |
| ADR-006 | llama.cpp + Qwen2.5 for local summarization | accepted |
| ADR-007 | Russian-first, English added later (auto-detect option) | partially superseded by ADR-012 (language mechanism only; Russian-first focus stands) |
| ADR-008 | Persisted meeting library (SQLite), reference-only audio, auto-save | accepted |
| ADR-009 | Structured meeting MFU (full set), editable | accepted |
| ADR-010 | Two-pane shell; manual Transcribe/MFU triggers; colored bubbles from M2 | accepted |
| ADR-011 | Settings: in-app model management, theming, i18n, English-default UI | accepted |
| ADR-012 | Transcription language is always auto-detected, never chosen (supersedes ADR-007 on the language mechanism) | accepted |
| ADR-013 | Isolate the diarization engine in a child process | accepted |
| ADR-014 | Streaming coexists with Meeting's batch-accuracy pipeline | accepted |
| ADR-015 | Live translation reuses the summary LLM and runs concurrently on a single-flight queue | accepted |
<!-- sdd-index-sync:end decisions -->

## Traceability

Feature-level tracking — requirements, tasks, scenarios, and their status —
lives in TaskPilot (project key **WP**), not in `docs/`. This index maps
documents and decisions only; it does not attempt feature-to-milestone
traceability.
