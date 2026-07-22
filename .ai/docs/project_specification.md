# WhisperPilot Project Specification

## Project Purpose

WhisperPilot is a macOS desktop application that transcribes local audio and
video files offline. It prioritizes accurate, Russian-first batch
transcription, speaker attribution, editable transcripts, and local AI meeting
notes. Core processing must remain on-device; live capture and cloud processing
are out of scope.

## User Role And Recurring Duties

The user is the product owner and lead developer. Recurring AI-assisted work
includes product and architecture decisions, React/Tauri/Rust implementation,
test writing and review, documentation maintenance, bug investigation, and
release preparation.

## AI Tool Surface

- **Mode:** single-tool.
- **Active tool:** Codex.
- **Instruction landscape:** the root `AGENTS.md` contract and the project-local
  AI-governance materials under `.claude/`; `.manifesto/` is the adopted
  framework source used to compose and review that landscape.

## Known Capability Triggers

- Non-trivial product implementation, refactoring, and bug fixes.
- Code and instruction-artifact review.
- Validation and task closure.
- Product documentation maintenance.
- AI-governance and instruction-system maintenance.

## Domain Vocabulary

Use `docs/glossary.md` as the authoritative vocabulary source. Core terms
include meeting, transcript, diarization, MFU section, local model, and
workspace.

## Authoritative Local Sources

1. `AGENTS.md` for project-wide AI operating policy.
2. `docs/INDEX.md` for the live documentation map.
3. `docs/idea.md`, `docs/architecture.md`, `docs/design.md`, `docs/testing.md`,
   and `docs/roadmap.md` for product and engineering authority.
4. `docs/decisions/` for accepted architectural decisions.
5. Source code and tests for current implementation behavior.

## Quality Expectations And Preferred Workflows

- Preserve the offline, local-first privacy model.
- Use TaskPilot for non-trivial product and product-documentation work; AI
  governance maintenance is TaskPilot-exempt as defined by `AGENTS.md`.
- Classify and route non-trivial work before edits.
- Use TDD provenance for non-trivial logic and run routed validation before
  closure.
- Commit locally only when requested; never push without explicit current-turn
  approval.

## Accepted Assumptions

- Codex is the only AI tool required immediately.
- The recurring duties listed above are the instruction system's initial
  priority areas.
- Existing product documentation and `AGENTS.md` are sufficiently current to
  serve as the initial authority set.

## Open Profile Gaps

- Whether external best-practice research may be used for future profile or
  instruction-system updates. It was not needed for this initial profile.
