---
name: validate
description: Run local CI-equivalent checks (lint, format, typecheck, tests, coverage, Rust checks) on the current state of the working tree and report pass/fail.
---

# Skill: validate

## Purpose

Run a caller-declared fast post-edit check plan and report auditable
pass/fail/block evidence before the independent `test-runner` gate.

## When This Skill Applies

Use when:
- a pipeline step has finished writing code and needs a quick post-edit check
- the task modified code and needs fast post-edit feedback
- the pipeline explicitly calls this skill as part of its Steps

Do not use:
- as a replacement for the dedicated `test-runner` agent
- for tasks that write no code (triage, documentation-only, instruction-only)

## Check Selection

The caller passes the implementation artifact, exhaustive changed-file list,
the pipeline's file-to-check mapping result, and a space-separated `checks`
value. Missing changed files or a check-plan mismatch is `BLOCKED`.

| Check | Command | When to run |
|-------|---------|-------------|
| `lint` | `npm run lint` | Any TS/TSX change |
| `format` | `npm run format:check` | Any source change |
| `tsc` | `npx tsc --noEmit` | Any TS/TSX change |
| `vitest` | `npm run test:run` | Any TS/TSX change |
| `coverage` | `npm run test:coverage` | After adding new tests or modifying tested code |
| `build` | `npm run build` | Any TS/TSX change |
| `clippy` | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | Any Rust change |
| `rusttest` | `cargo test --manifest-path src-tauri/Cargo.toml` | Any Rust change |
| `cargobuild` | `cargo build --manifest-path src-tauri/Cargo.toml` | Any Rust change |
| `audit` | `cargo audit --manifest-path src-tauri/Cargo.toml` | Any `Cargo.lock` or dependency change |

For routed work, an absent or empty `checks` value is `BLOCKED`; do not invent a
default. Ad-hoc command execution is outside this skill.

### Check name validation

Before running, validate each requested check name against the table above. An
unknown or empty name is `BLOCKED`; do not run a partial plan.

## Rules

### 1. Run Each Check and Report

Run each requested check sequentially. For each:
- print the check name and the command
- capture stdout + stderr
- report `PASS`, `FAIL`, or `BLOCKED`

### 2. Stop on First Failure

If an executed check finds a code defect, report `FAIL` and stop. Missing
tooling, invalid input, timeout, signal termination, or infrastructure failure
is `BLOCKED`, not `FAIL`. List all checks not run after fail-fast.

### 3. Mutation Boundary

Do not intentionally edit source, tests, configuration, or TaskPilot files and
never run auto-fix commands. Named validation commands may create build,
coverage, audit, or cache outputs. Compare `git status --short` before and after;
unexpected tracked-file changes are `BLOCKED`.

### 4. Manual UI Verification Selection

From the exhaustive changed-file list, emit `required` when a visual UI or
interaction surface changed, otherwise `skipped — no visual UI or interaction
surface changed`. When required, name the pipeline's `Manual UI verification
record`; this skill selects the requirement but does not perform the manual
check.

## Output Contract

Emit:

`Skill: validate - output below`

`Overall status: PASS / FAIL / BLOCKED`

| Check | Exact command | Result | Exit / duration | Evidence |
|---|---|---|---|---|

Then list `Not run after fail-fast` and emit:

`Manual UI verification: required — Manual UI verification record / skipped — no visual UI or interaction surface changed`

Include the requested-versus-run check list and the pre/post working-tree
comparison. `PASS` requires every requested check to pass and no unexpected
tracked-file mutation.
