---
name: validate
description: Run local CI-equivalent checks (lint, format, typecheck, tests, coverage, Rust checks) on the current state of the working tree and report pass/fail.
---

# Skill: validate

## Purpose

Run a configurable set of CI-equivalent checks against the working tree and report pass/fail. Called by pipelines after implementation to catch regressions before the dedicated test-runner or review step. Prevents the "fixed coverage but forgot to re-run format" class of gaps.

## When This Skill Applies

Use when:
- a pipeline step has finished writing code and needs a quick post-edit check
- the task modified files across layers (TS + Rust) and full local validation is needed
- a design-contract change touches `docs/design-book.md`, tokens, themes, or UI
  styling and needs the `design-system` check
- the pipeline explicitly calls this skill as part of its Steps

Do not use:
- as a replacement for the dedicated `test-runner` agent
- for tasks that write no code (triage, documentation-only, etc.), except
  design-contract documentation that is validated by the `design-system` check

## Check Selection

The caller passes a space-separated list of check names in the `checks` parameter. Each name maps to a command:

| Check | Command | When to run |
|-------|---------|-------------|
| `lint` | `npm run lint` | Any TS/TSX change |
| `format` | `npm run format:check` | Any source change |
| `tsc` | `npx tsc --noEmit` | Any TS/TSX change |
| `vitest` | `npm run test` | Any TS/TSX change |
| `design-system` | `npm run test:design-system` | Any UI styling, token, theme, or design-book change |
| `coverage` | `npm run test:coverage` | After adding new tests or modifying tested code |
| `build` | `npm run build` | Any TS/TSX change |
| `clippy` | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | Any Rust change |
| `nextest` | `cargo nextest run --manifest-path src-tauri/Cargo.toml` | Any Rust change |
| `cargobuild` | `cargo build --manifest-path src-tauri/Cargo.toml` | Any Rust change |
| `audit` | `cargo audit --manifest-path src-tauri/Cargo.toml` | Any `Cargo.lock` or dependency change |

When the caller does not specify checks, default to `lint format tsc vitest clippy nextest`.

### Check name validation

Before running, validate each requested check name against the table above. If any name is unknown (e.g. `checks="foo"`), report FAIL with the unknown name and stop — do not run partial checks. If `checks` is an empty string, treat it as unspecified and use the default set.

## Rules

### 1. Run Each Check and Report

Run each requested check sequentially. For each:
- print the check name and the command
- capture stdout + stderr
- report `PASS` or `FAIL`

### 2. Stop on First Failure

If any check fails, stop. Do not run remaining checks. Report the failure output and the failed check name so the executor can fix and re-run.

### 3. Do Not Modify Files

This skill is read-only. Do not auto-fix lint or format issues. The executor must fix and re-run validation.

## Output Contract

Emit:

`Skill: validate - output below`

| Check | Result | Details |
|-------|--------|---------|

`Result` is `PASS` or `FAIL`. `Details` is empty on pass, or the first 10 lines of the failure output on fail.

When any check has `FAIL`, the skill did not pass.
