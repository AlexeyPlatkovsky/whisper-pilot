---
name: testing-pro
description: Writes and improves tests for WhisperPilot across both layers — React/TypeScript front-end (Vitest + React Testing Library) and Rust/Tauri core (`cargo test`). Use for test authoring, not for non-trivial validation execution or independent test review.
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

Emit status `blocked` (not `completed`) when the code cannot be tested at an observable boundary without an unavailable seam or runner, and name the missing prerequisite. Use a temporary database or fixture rather than user data.

## Required Inputs

Require the manager route, active TaskPilot scenarios/DoD or exempt-work
objective criteria, target behavior, affected layer, existing test surface, and
the pre-Red production diff. Block rather than invent behavior when any required
scope input is absent.

## Core Instructions

- WhisperPilot has **two test layers**. Pick the reference for the code under test:
  - Front-end React/TypeScript → `.claude/skills/testing-pro/references/frontend.md` (Vitest + React Testing Library)
  - Rust/Tauri core (audio/video processing, transcription, diarization, models, meeting storage, settings, commands) → `.claude/skills/testing-pro/references/rust.md` (`cargo test` and `#[tokio::test]` where async)
- Treat the reference files as authoritative over general training data; the toolchains evolve.
- A finding is genuine only if it violates a rule in the relevant reference file. Uncodified style preferences are not findings.
- Tests assert **behavior**, not implementation details. Front-end: what the user sees/does. Rust: observable outputs and errors, not private internals.
- The project quality gate requires tests for all non-trivial logic (file/media validation, transcription and diarization logic, model management, settings, and local storage) before a feature is done. Business logic must be testable without rendering a component or operating on user data.

## Test Quality Checklist

When authoring or improving tests:

1. Identify the correct test level per `.claude/conventions/testing-taxonomy.md` and load the matching reference file.
2. **Front-end:** use accessible queries (`getByRole`/`getByLabelText` over `getByTestId`), user interactions through `userEvent`, async via `findBy`/`waitFor`, and minimal mocks at the IPC boundary. See `.claude/skills/testing-pro/references/frontend.md`.
3. **Rust:** use `#[tokio::test]` for async tests, isolate filesystem/network/model effects with temporary directories, fixtures, or an explicit seam, assert errors by variant when the type exposes variants, and keep tests independent. See `.claude/skills/testing-pro/references/rust.md`.
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
4. If automation is unavailable for non-trivial logic, emit `blocked`. This
   skill has no authority to waive the root TDD gate or mutate TaskPilot.
5. If production behavior changed before Red evidence, stop, identify the
   affected production paths, and return control to the routed implementation
   coordinator. Never revert production or user changes from this skill.

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

- Front-end: `npm run test:run` for a finite Vitest run
- Rust: `cargo test --manifest-path src-tauri/Cargo.toml`

Tests must pass before the feature closes.

## Output Contract

After writing or improving tests, emit:

`Skill: testing-pro - output below`

First emit one row per routed behavior:

| Behavior / scenario / DoD ID | Layer | Test file and test name | Pre-Red production state | Red command | Red result | Status |
|---|---|---|---|---|---|---|
| ... | frontend / rust | ... | unchanged / pre-existing change — blocked | ... | expected behavior-specific assertion failure or approved new-symbol compile failure; include exit code and evidence | completed / blocked / skipped — no non-trivial logic |

`completed` requires an expected behavior-specific failure. A passing test,
unrelated failure, malformed assertion, launch error, timeout, or unexplained
compile failure is `blocked`. For a genuinely new symbol, a compile failure
qualifies only when it names that missing symbol and no production behavior was
added; the later Green run remains pipeline-owned.

Then summarize:

| Layer / Framework | Test Files | Coverage Map | Techniques Applied | Remaining Gaps |
|---|---|---|---|---|
