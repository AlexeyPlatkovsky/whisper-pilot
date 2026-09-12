---
name: testing
description: Design or author WhisperPilot tests, including isolated Red evidence before production logic changes.
---

# Testing

## Apply when

Use for test design, TDD, regression coverage, or changes to Vitest or Rust tests. Use `test-runner`, not this skill,
when the task is only to execute final validation.

## Invariants

- Test observable behavior at the lowest boundary that can prove it.
- For non-trivial production logic, capture a focused failing test before the first behavior edit.
- A valid Red result fails for the expected missing or incorrect behavior, not setup or unrelated errors.
- Use fixtures, temporary directories, and explicit seams; never use private user data.
- Cover the happy path, relevant boundaries, and at least one meaningful failure path.
- Trace tests to a TaskPilot scenario, DoD item, defect, or named invariant.
- Do not add test dependencies or global quality thresholds without explicit approval.

## Project boundaries

- Front end: Vitest, React Testing Library, `userEvent`, and typed `src/ipc.ts` mocks.
- Rust: `cargo test`, `#[tokio::test]` for async code, and isolated filesystem or storage.
- IPC changes need Rust serialization or command coverage and the matching TypeScript wrapper coverage.
- WKWebView or native-window behavior needs real macOS Tauri evidence in addition to lower-level tests.

## Test authoring agent

Use `test-author` for recurring non-trivial TDD work when independent context reduces implementation bias. Pass exact
behavior, allowed test and fixture paths, current production state, and the focused command. Run it sequentially before
production implementation.

## Commands

- Front end: `npm run test:run -- <focused args>`
- Rust: `cargo test --manifest-path src-tauri/Cargo.toml <focused args>`

## Result

Report behavior, test path and name, command, Red or Green result, and remaining coverage gaps.
