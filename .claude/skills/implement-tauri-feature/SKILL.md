---
name: implement-tauri-feature
description: Implement routed feature or confirmed bug-fix scope in WhisperPilot's Tauri v2, React, TypeScript, and Rust surfaces after route-specific gates complete.
---

# Skill: implement-tauri-feature

## Purpose

Implement routed feature or confirmed-bug scope across the React front-end
and/or Rust (Tauri) core. When a new user decision is required, stop with
`Blocked` and return control to the invoking pipeline; do not pause for
mid-implementation discussion. The output gates downstream validation and
documentation maintenance.

## When This Skill Applies

Use when:
- the `implement-feature` route is ready (design decisions resolved and
  route-specific prerequisites present), or the `fix-bug` route has qualifying
  triage/root-cause evidence
- the scope involves React/TypeScript UI, Rust commands/logic, Tauri
  windowing/plugins, local audio/video processing, transcription, diarization,
  model management, settings, or meeting storage

Do not use when:
- design decisions are still open — stop and report the unresolved decision
- the manager routing plan (`Manager: manager - output below`) is absent from the conversation
- the request is test authoring or review with no feature implementation

This skill is pipeline-only. The task-baseline snapshot is bound to the manager
Route run and captured before any task-authored test or production edit; it
lists every existing working-tree path and status, or `clean`. Before edits,
both routes require the manager Route run, completed Git gate,
reload-verified `in_progress` lifecycle artifact,
task-baseline snapshot, approved scope/DoD/scenarios, and `testing-pro`
artifact. `implement-feature` additionally requires `verify-readiness: Ready`
and any applicable confirmed brainstorm artifact. `fix-bug` instead requires
its pipeline's qualifying triage/reproduction/root-cause artifact. Any other
route is `Blocked`.

## Before Implementing

1. Confirm the manager's routing plan is present in the conversation.
2. Load `docs/idea.md` for the relevant feature's design intent.
3. Follow the architecture documentation decision in the manager artifact:
   - If required, load only the focused sections of `docs/architecture.md` named by the manager.
   - If skipped, do not load architecture docs. If scope changes, stop all edits
     and return for updated tracked scope, readiness, design, test evidence, and
     manager routing.
   - If the manager artifact lacks an architecture documentation decision, stop and report the missing decision as a blocker.
4. Confirm open design decisions are resolved. If any remain, stop and report the unresolved decision as a blocker.
5. Load the relevant convention files from `.claude/conventions/react-tauri/` for the touched surface: windowing, IPC/permissions, state, accessibility, performance, or cross-platform.
   - Also load `desktop-platform-scope.md` when changing app icons, bundle assets, dev ports, window dimensions, or UI design variants.

## Implementation Steps

1. **Scope** — Record the caller-supplied task baseline (pre-existing changed
   files), then state the planned files and every routed scenario/DoD criterion
   before writing code.
2. **Design data flow** — Identify component-local and shared React state, the `src/ipc.ts` wrapper change if IPC is touched, and the Rust-core versus front-end boundary. Do not introduce Zustand, TanStack Query, or generated bindings without an approved routed decision.
3. **Confirm required test evidence** — Require the exact labeled
   `testing-pro` behavior table and summary. It is malformed if a routed
   behavior row or required column is absent, an ID is duplicated/unmapped,
   Red command/result/exit evidence is empty, or a status is outside that
   skill's enum. For non-trivial logic every routed behavior row must be
   `completed`; accept `skipped — no non-trivial logic` only when the manager
   artifact makes the same declaration. Stop on malformed, `blocked`, or
   unjustified skipped evidence.
4. **Implement Rust side (if any)** — Add commands as focused wrappers over testable functions; register the command; keep serializable DTOs explicit; and add only the capability permissions a new plugin or non-default scope requires.
5. **Implement React side (if any)** — Extend `src/ipc.ts` for changed command shapes before using it from UI; add accessibility; keep business logic in plain TS modules.
6. **Refactor within the approved scope** — Do not change observable behavior beyond the routed item.

 ## Quality Requirements (non-negotiable)

- Every front-end IPC/plugin call must use the typed wrapper in `src/ipc.ts`; new plugin or non-default capability usage must have a matching, minimally scoped capability permission.
- The pipeline's validation step owns build and test execution.

## Output Contract

Emit before the validation step:

`Skill: implement-tauri-feature - output below`

| Status | All Files Changed | Layer / type | Test-Evidence Artifact | Implementation Notes | Blockers |
|--------|-------------------|--------------|------------------------|----------------------|----------|

| Scenario / DoD ID | Implemented behavior | Files / symbols | Result |
|---|---|---|---|

`Complete` requires full routed-scope implementation and no blockers;
`Blocked` means required authority/input/dependency is absent; `Failed` means an
attempt could not complete and names the remaining tree state. `All Files
Changed` is exhaustive and repository-relative, including tests, permissions,
configuration, assets, styles, and manifests. Derive it by reconciling the
task baseline, planned files, testing-pro changes, actual edits, and the
task-scoped diff; list pre-existing unrelated changes separately rather than as
task files. Every routed scenario/DoD row must map to implemented behavior and
files/symbols with `Result: implemented`; otherwise status is `Blocked`.
Build/test results remain pipeline-owned.

When UI or interaction surfaces are touched, also include:

**UI Interaction Contract** — default/initial state; click-versus-drag behavior;
keyboard behavior; sizing and resize bounds; empty/loading/error states; and
user-visible outcome. Every field requires an explicit value or `N/A —
unchanged/not applicable`.
