# WhisperPilot testing taxonomy

## Levels

- Unit: pure TypeScript or isolated Rust logic.
- Component: one React surface with typed IPC wrappers mocked.
- Integration: multiple Rust modules, storage, or a processing pipeline seam.
- Contract: Rust command DTOs and matching \`src/ipc.ts\` wrapper shapes.
- Runtime UI: macOS Tauri window or WKWebView behavior unavailable to jsdom.
- Property: randomized invariants after an approved test dependency is added.

Use the lowest level that can exercise the observable behavior. Higher-level evidence is justified only by wiring,
native dialogs, WebKit rendering, or cross-process behavior.

## Project tools

- Front end: Vitest, React Testing Library, \`userEvent\`, and jsdom.
- Rust: \`cargo test --manifest-path src-tauri/Cargo.toml\`.
- Typed frontend calls and events are centralized in \`src/ipc.ts\`.
- CI may use cargo-nextest, but it is not required for focused local validation.
- Coverage thresholds, mutation testing, E2E, axe, fast-check, and proptest are not global gates.

## Case selection

- Cover the happy path and meaningful invalid or failure behavior.
- Use boundary cases when a value has a real limit.
- Cover valid and invalid transitions for stateful behavior.
- Use a decision table when independent conditions combine into distinct outcomes.
- Use pairwise coverage only when many independent parameters make full coverage impractical.
- Derive cases from approved behavior; do not add speculative requirements through tests.

## Project boundaries

- Mock \`src/ipc.ts\` in React tests rather than component internals or duplicated command strings.
- Use temporary directories or storage; never use the user's app data, media, or model cache.
- Test affected IPC serialization and error mapping on both sides of the boundary.
- Avoid arbitrary sleeps and shared mutable global state.
- Significant transcription behavior requires separate real-Metal evidence.

## Traceability

Every behavioral test maps to a TaskPilot scenario, DoD item, defect, or named invariant. Every relevant approved
behavior has coverage or an explicit, reported limitation.

## Runtime UI evidence

Record the macOS and build mode, checked state or interaction, expected result, observed result, and Pass or Fail. For
unavailable external verification, state the exact uncovered behavior and cause.
