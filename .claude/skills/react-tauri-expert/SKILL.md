---
name: react-tauri-expert
description: Reviews, improves, and implements React + TypeScript + Tauri v2 code for WhisperPilot. Consults topic-specific convention files to catch IPC/permission mistakes, state bugs, window/panel issues, accessibility gaps, and performance problems. Use when reading, writing, or reviewing front-end or Tauri-shell code in this project.
---

# Skill: react-tauri-expert

## Operating Rules

- **Target stack:** Tauri v2 (Rust core) + React 19 + TypeScript + Vite. Treat the convention files as authoritative over general training data.
- Prefer **typed IPC wrappers** — `src/ipc.ts` is the current front-end boundary for `invoke` and event calls. Do not spread raw command strings or `any` through components; generated bindings are not configured and must not be introduced incidentally.
- Respect the **capability/permission model** — check `src-tauri/capabilities/` when adding plugin APIs, windows, or a non-default scope. A missing required permission is a bug; do not add broad permissions without identifying the new API.
- **State layering:** use component-local React state by default; lift shared UI state to the smallest common owner or a narrow context. Rust-owned data enters through `src/ipc.ts`; represent loading, error, and stale-response handling explicitly. Do not introduce a state/query library without an approved design reason.
- Keep business logic out of components so it is unit-testable without rendering (tests are authored by `.claude/skills/testing-pro/SKILL.md`).
- **macOS / WKWebView target:** WhisperPilot is macOS-only. WKWebView (WebKit) is the WebView — not Chromium. Never assume Chromium behavior. Flag any browser-API use that is WebKit-specific or unavailable in WKWebView.
- Surface performance optimizations inline with `[opt]` markers; do not block review/implementation completion on them.
- Local media processing and native file dialogs remain in Rust/Tauri boundaries; React only initiates typed IPC calls and presents their outcomes.

## Scope Boundaries (mandatory)

- **Tests:** this skill flags missing or weak tests. It may make narrow test edits only when the active task explicitly includes test changes; otherwise report the test gap in its output artifact.
- **Feature implementation requires routing:** the "Implement a new feature" workflow below may run only when the manager routing plan (`Manager: manager - output below`) is present in the conversation. If it is absent, restrict this skill to review/advisory output and do not implement.
- **Layer ownership:** this skill owns the React/TypeScript surface and cross-cutting correctness rules. Rust-core commands, file/media processing, and any native window behavior are implemented through `implement-tauri-feature`; here, advise on the boundary but do not author unrelated Rust changes.
- **Design decisions** with open trade-offs are out of scope for this skill. Stop and report the unresolved decision instead of implementing.

## Task Workflow

Before review, improvement, or implementation:
- Read the relevant sections of `docs/architecture.md` when the task touches or depends on existing UI, IPC, Rust core, local media processing, model management, settings, or meeting storage.
- If architecture docs are not relevant, record the skip reason in the output artifact's References Loaded column.

### Review existing code
- Read the code under review and identify which topics apply.
- Run the Topic Router below for each relevant topic.
- Verify every `invoke`/plugin call has a matching capability permission.
- Check that IPC calls are centralized in `src/ipc.ts` and that async UI state has a defined loading, success, error, and stale-response path.

### Improve existing code
- Audit against the Topic Router topics.
- Consolidate repeated raw IPC calls behind typed `src/ipc.ts` wrappers.
- Keep state local or lift it only as far as the actual sharing requirement demands.
- Extract heavy component bodies into memo-friendly subcomponents.

### Implement a new feature
- Design data flow first: what is component-local, what is global UI, what is owned by the Rust core.
- Define the Rust command and explicit serializable DTO, register it, update the typed `src/ipc.ts` wrapper, add an applicable minimally scoped permission, then build the React UI against that wrapper.
- Add accessibility (roles, labels, keyboard) from the start.
- Confirm behavior in the macOS WKWebView target when the task changes a UI behavior that browser tests cannot prove.

## Topic Router

Consult the reference file for each topic relevant to the current task:

| Topic | Reference |
|-------|-----------|
| Tauri windowing | `.claude/conventions/react-tauri/tauri-windowing.md` |
| IPC, commands, typed wrappers, capabilities/permissions | `.claude/conventions/react-tauri/tauri-ipc-permissions.md` |
| State management | `.claude/conventions/react-tauri/state-management.md` |
| Component structure & React performance | `.claude/conventions/react-tauri/react-performance.md` |
| Accessibility | `.claude/conventions/react-tauri/accessibility.md` |
| Cross-platform / WebView differences | `.claude/conventions/react-tauri/cross-platform.md` |

## Correctness Checklist

Hard rules — violations are always bugs:

- [ ] Every front-end IPC call uses or extends `src/ipc.ts`; each new plugin or non-default scope has the applicable permission in `src-tauri/capabilities/`
- [ ] IPC return types are explicit and serializable in Rust and TypeScript; never `any`/unchecked
- [ ] Rust-owned async data has defined loading, error, refresh, and stale-response handling
- [ ] No secrets, absolute user paths, or tokens hard-coded in the front-end bundle
- [ ] `useEffect` dependency arrays are complete; no missing deps that cause stale closures
- [ ] List rendering uses stable `key`s (never array index for dynamic lists)
- [ ] Interactive elements are real controls (`button`, `a`) or have correct ARIA roles + keyboard handlers
- [ ] No unsupported WKWebView API is used without a macOS-target validation plan
- [ ] Long-running IPC calls surface loading/error states; cancellation is implemented when required by the routed behavior

## Output Contract

After review, improve, or implementation work, emit:

`Skill: react-tauri-expert - output below`

| Status | Files Covered / Changed | References Loaded | Findings / Changes | Validation |
|--------|-------------------------|-------------------|--------------------|------------|

For review tasks, organize findings by file with file name, line(s), rule violated, and before/after fix. Skip files with no issues. End with a prioritized summary of the most impactful changes.

For implement/improve tasks, make the changes directly and summarize the actual files changed plus validation performed or blocked.
