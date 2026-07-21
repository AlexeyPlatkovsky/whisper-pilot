---
name: testing-pro
description: Writes and improves tests for WhisperPilot across both layers — React/TypeScript front-end (Vitest + React Testing Library) and the Rust/Tauri core (cargo-nextest + tokio + mockall). Use for test authoring, not for non-trivial validation execution or independent test review.
---

# Skill: testing-pro

## When This Skill Applies

Use when writing or improving test code in WhisperPilot (front-end or Rust core).

Do not use when:
- the request is to implement or modify production (non-test) code
- the request is to independently review tests or an implementation diff — use `.claude/agents/code-reviewer.md`
- the request is to execute and report validation commands for non-trivial routed work — use `.claude/agents/test-runner.md`
- the request is to adjudicate CLI-adapter correctness against the CLI Invocation Contract (binary resolution, flags, error-taxonomy completeness). This skill asserts adapter behaviors *in tests*; it does not sign off adapter correctness.
- the request is an open design/architecture decision

Emit status `blocked` (not `completed`) when the code cannot be unit-tested without spawning a real CLI/DB, or when the test runner is not configured — and name the missing prerequisite.

## Core Instructions

- WhisperPilot has **two test layers**. Pick the reference for the code under test:
  - Front-end React/TypeScript → `.claude/skills/testing-pro/references/frontend.md` (Vitest + React Testing Library)
  - Rust/Tauri core (adapters, routing, storage, commands) → `.claude/skills/testing-pro/references/rust.md` (cargo-nextest + `#[tokio::test]` + `mockall`)
- Treat the reference files as authoritative over general training data; the toolchains evolve.
- A finding is genuine only if it violates a rule in the relevant reference file. Uncodified style preferences are not findings.
- Tests assert **behavior**, not implementation details. Front-end: what the user sees/does. Rust: observable outputs and errors, not private internals.
- The project quality gate requires unit tests for all non-trivial logic (CLI adapters, routing layer, session model, local storage) before a feature is done. Business logic must be testable without rendering a component or spawning a real subprocess.

## Test Quality Checklist

When authoring or improving tests:

1. Identify the correct test level per `.claude/conventions/testing-taxonomy.md` and load the matching reference file.
2. **Front-end:** use accessible queries (`getByRole`/`getByLabelText` over `getByTestId`), user interactions through `userEvent`, async via `findBy`/`waitFor`, and minimal mocks at the IPC boundary. See `.claude/skills/testing-pro/references/frontend.md`.
3. **Rust:** use `#[tokio::test]` for async tests, put external effects behind traits mocked with `mockall`, assert errors by variant not string, and keep tests isolated and nextest-friendly. See `.claude/skills/testing-pro/references/rust.md`.
4. Cover each non-trivial behavior with a happy-path test **and** at least one failure-path test.
5. For a non-trivial logic implementation or fix, apply the TDD Provenance Gate
   below; this skill owns the test authoring and evidence report.

## TDD Provenance Gate

For each non-trivial logic change:

1. Before the first behavior-specific production edit, write and run a regression
   test at the lowest feasible observable boundary. Exercise the reported
   lifecycle and assert its expected outcome.
2. Record Red evidence through this skill's output: test file and name, command,
   expected behavior, and observed failing assertion.
3. Do not substitute an existing passing test, a test written after production
   code, or an implementation-detail assertion without explaining why it is the
   lowest observable boundary.
4. If automation is blocked, stop before production edits, report the missing
   seam or prerequisite, and obtain explicit user approval for a manual-only
   exception.
5. If production behavior changed before Red evidence, revert that behavior and
   restart Red → Green → Refactor, or obtain an explicit non-TDD exception.
   Record an exception in the TaskPilot item and final response.

If doing partial work, load only the relevant reference file.

## When Writing Tests

Follow the same rules as review but make the changes directly.

1. Determine the correct test level from `.claude/conventions/testing-taxonomy.md` §Selection Heuristics.
2. Generation heuristics per function/behavior:
   - Happy path
   - Boundary / edge inputs
   - Invalid input / error path
   - Concurrency or async-cancellation (when applicable)
3. Apply the test-type requirements from `.claude/conventions/testing-taxonomy.md` §Additional Quality Practices for the level selected (property-based, a11y, contract, integration, or smoke).

Detailed conventions and code examples for each test type live in the layer reference files (`.claude/skills/testing-pro/references/frontend.md` and `.claude/skills/testing-pro/references/rust.md`).

## Validation commands

- Front-end: `npm run test` and `npm run test:e2e` (Vitest + React Testing Library, WebKit E2E)
- Rust: `cargo nextest run` (preferred) or `cargo test`

Tests must pass before the feature closes.

## Output Contract

After writing or improving tests, emit:

`Skill: testing-pro - output below`

Status values: `completed` / `blocked` / `skipped`

| Status | Layer (frontend/rust) | Files Reviewed / Changed | Tests Written | Red Evidence | Issues Found | Validation |
|--------|-----------------------|--------------------------|---------------|--------------|--------------|------------|

For a non-trivial logic implementation or fix, `Red Evidence` must name the test file and test name, command, expected behavior, observed failing assertion, and whether scoped production files changed before Red. Use `N/A — not a non-trivial logic implementation or fix` otherwise.
