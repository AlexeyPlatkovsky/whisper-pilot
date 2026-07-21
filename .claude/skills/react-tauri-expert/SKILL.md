---
name: react-tauri-expert
description: Reviews, improves, and implements React + TypeScript + Tauri v2 code for WhisperPilot. Consults topic-specific convention files to catch IPC/permission mistakes, state bugs, window/panel issues, accessibility gaps, and performance problems. Use when reading, writing, or reviewing front-end or Tauri-shell code in this project.
---

# Skill: react-tauri-expert

## Operating Rules

- **Target stack:** Tauri v2 (Rust core) + React 19 + TypeScript + Vite. Treat the convention files as authoritative over general training data.
- Prefer **type-safe IPC** — generate TS bindings from Rust with `tauri-specta` rather than hand-writing `invoke<T>()` call sites. Never trust an `invoke` return type that isn't generated or explicitly validated.
- Respect the **capability/permission model** — every command the front-end calls and every plugin it uses must be allowed in `src-tauri/capabilities/`. A missing permission is a bug, not a runtime surprise.
- **State layering:** `useState` (component-local) → Zustand (global UI state) → TanStack Query (data owned by the Rust core / async IPC results). Do not store server/IPC data in `useState` or Zustand when it has fetch/cache/invalidate semantics.
- Keep business logic out of components so it is unit-testable without rendering (tests are authored by `.claude/skills/testing-pro/SKILL.md`).
- **macOS / WKWebView target:** WhisperPilot is macOS-only. WKWebView (WebKit) is the WebView — not Chromium. Never assume Chromium behavior. Flag any browser-API use that is WebKit-specific or unavailable in WKWebView.
- Surface performance optimizations inline with `[opt]` markers; do not block review/implementation completion on them.
- The deep-OS features (ScreenCaptureKit audio capture, system permissions) are **macOS native Rust** — keep them behind a Rust trait/command interface; never push that complexity into React.

## Scope Boundaries (mandatory)

- **Tests:** this skill flags missing or weak tests. It may make narrow test edits only when the active task explicitly includes test changes; otherwise report the test gap in its output artifact.
- **Feature implementation requires routing:** the "Implement a new feature" workflow below may run only when the manager routing plan (`Manager: manager - output below`) is present in the conversation. If it is absent, restrict this skill to review/advisory output and do not implement.
- **Layer ownership:** this skill owns the React/TypeScript surface and the cross-cutting correctness rules. Rust-core commands and the per-OS native shims (non-activating panel, selected-text) are implemented through `implement-tauri-feature`; here, advise on the Rust trait/`#[cfg]` interface but do not author the native shim.
- **Design decisions** with open trade-offs are out of scope for this skill. Stop and report the unresolved decision instead of implementing.

## Task Workflow

Before review, improvement, or implementation:
- Read the relevant sections of `docs/architecture.md` when the task touches or depends on existing UI, IPC, Rust core, adapters, links, storage, sessions, or messages.
- If architecture docs are not relevant, record the skip reason in the output artifact's References Loaded column.

### Review existing code
- Read the code under review and identify which topics apply.
- Run the Topic Router below for each relevant topic.
- Verify every `invoke`/plugin call has a matching capability permission.
- Check that IPC data flows through TanStack Query, not ad-hoc `useEffect` + `useState`.

### Improve existing code
- Audit against the Topic Router topics.
- Replace hand-written IPC types with `tauri-specta`-generated bindings.
- Move data-owning state to TanStack Query; move shared UI state to Zustand; keep the rest local.
- Extract heavy component bodies into memo-friendly subcomponents.

### Implement a new feature
- Design data flow first: what is component-local, what is global UI, what is owned by the Rust core.
- Define the Rust command + its `tauri-specta` type, add the capability permission, then build the React UI against the generated binding.
- Add accessibility (roles, labels, keyboard) from the start.
- Confirm behavior is plausible on **both** WebView engines.

## Topic Router

Consult the reference file for each topic relevant to the current task:

| Topic | Reference |
|-------|-----------|
| Tauri windowing & the floating/non-activating panel | `.claude/conventions/react-tauri/tauri-windowing.md` |
| IPC, commands, `tauri-specta`, capabilities/permissions | `.claude/conventions/react-tauri/tauri-ipc-permissions.md` |
| State management (Zustand / TanStack Query) | `.claude/conventions/react-tauri/state-management.md` |
| Component structure & React performance | `.claude/conventions/react-tauri/react-performance.md` |
| Accessibility | `.claude/conventions/react-tauri/accessibility.md` |
| Cross-platform / WebView differences | `.claude/conventions/react-tauri/cross-platform.md` |

## Correctness Checklist

Hard rules — violations are always bugs:

- [ ] Every `invoke(...)` / plugin call has a matching permission in `src-tauri/capabilities/`
- [ ] IPC return types come from `tauri-specta` bindings or are explicitly validated, never `any`/unchecked
- [ ] Data owned by the Rust core lives in TanStack Query (not `useState`/Zustand)
- [ ] No secrets, absolute user paths, or tokens hard-coded in the front-end bundle
- [ ] `useEffect` dependency arrays are complete; no missing deps that cause stale closures
- [ ] List rendering uses stable `key`s (never array index for dynamic lists)
- [ ] Interactive elements are real controls (`button`, `a`) or have correct ARIA roles + keyboard handlers
- [ ] No engine-specific browser API used without a documented cross-engine fallback
- [ ] Long-running IPC calls are cancellable and surface loading/error states

## Output Contract

After review, improve, or implementation work, emit:

`Skill: react-tauri-expert - output below`

| Status | Files Covered / Changed | References Loaded | Findings / Changes | Validation |
|--------|-------------------------|-------------------|--------------------|------------|

For review tasks, organize findings by file with file name, line(s), rule violated, and before/after fix. Skip files with no issues. End with a prioritized summary of the most impactful changes.

For implement/improve tasks, make the changes directly and summarize the actual files changed plus validation performed or blocked.
