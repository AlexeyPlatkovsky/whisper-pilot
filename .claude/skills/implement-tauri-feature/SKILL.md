---
name: implement-tauri-feature
description: Implement a Tauri v2 + React + TypeScript feature in WhisperPilot (React UI and/or Rust core) after routed test authoring and readiness gates complete.
---

# Skill: implement-tauri-feature

## Purpose

Implement a feature of the WhisperPilot desktop app across the React front-end and/or the Rust (Tauri) core. This skill runs in the working context, may interact with the user mid-task, and produces an output artifact that gates downstream validation and documentation maintenance.

## When This Skill Applies

Use when:
- a feature is ready to be implemented (design decisions are resolved, manager routing plan exists)
- the feature involves React/TypeScript UI, Rust commands/logic, Tauri windowing/plugins, local audio/video file processing, transcription, diarization, model management, settings, or meeting storage

Do not use when:
- design decisions are still open — stop and report the unresolved decision
- the manager routing plan (`Manager: manager - output below`) is absent from the conversation
- the request is test authoring or review with no feature implementation

## Before Implementing

1. Confirm the manager's routing plan is present in the conversation.
2. Load `docs/idea.md` for the relevant feature's design intent.
3. Follow the architecture documentation decision in the manager artifact:
   - If required, load only the focused sections of `docs/architecture.md` named by the manager.
   - If skipped, do not load architecture docs. If the implementation scope changes, stop and request an updated manager decision before loading them.
   - If the manager artifact lacks an architecture documentation decision, stop and report the missing decision as a blocker.
4. Confirm open design decisions are resolved. If any remain, stop and report the unresolved decision as a blocker.
5. Load the relevant convention files from `.claude/conventions/react-tauri/` for the touched surface: windowing, IPC/permissions, state, accessibility, performance, or cross-platform.
   - Also load `desktop-platform-scope.md` when changing app icons, bundle assets, dev ports, window dimensions, or UI design variants.

## Implementation Steps

1. **Scope** — State the files to be created or modified and the acceptance criteria before writing code.
2. **Design data flow** — Identify component-local and shared React state, the `src/ipc.ts` wrapper change if IPC is touched, and the Rust-core versus front-end boundary. Do not introduce Zustand, TanStack Query, or generated bindings without an approved routed decision.
3. **Confirm required test evidence** — For non-trivial logic, require the completed `Skill: testing-pro - output below` artifact before changing production code; `.claude/skills/testing-pro/SKILL.md` owns its procedure.
4. **Implement Rust side (if any)** — Add commands as focused wrappers over testable functions; register the command; keep serializable DTOs explicit; and add only the capability permissions a new plugin or non-default scope requires.
5. **Implement React side (if any)** — Extend `src/ipc.ts` for changed command shapes before using it from UI; add accessibility; keep business logic in plain TS modules.
6. **Refactor within the approved scope** — Do not change observable behavior beyond the routed item.

 ## Quality Requirements (non-negotiable)

- Every front-end IPC/plugin call must use the typed wrapper in `src/ipc.ts`; new plugin or non-default capability usage must have a matching, minimally scoped capability permission.
- The pipeline's validation step owns build and test execution.

## Output Contract

Emit before the validation step:

`Skill: implement-tauri-feature - output below`

| Status | Files Changed (FE/Rust) | Test-Evidence Artifact | Implementation Notes | Blockers |
|--------|-------------------------|------------------------|----------------------|----------|

`Status` is `Complete`, `Blocked`, or `Failed`. Build and test results belong to the pipeline's validation artifact, not this implementation artifact.

When UI or interaction surfaces are touched, also include:

**UI Interaction Contract** — default/initial state; keyboard behavior; sizing and resize
bounds when changed; empty/loading/error states; and user-visible outcome. Include drag or
window behavior only when the routed scope changes it.
