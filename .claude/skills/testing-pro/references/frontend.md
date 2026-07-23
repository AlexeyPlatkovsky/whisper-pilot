# Front-end Testing — Vitest + React Testing Library

Stack: **Vitest** (runner, Vite-native, fast), **React Testing Library** (RTL), `@testing-library/jest-dom`, `@testing-library/user-event`, `jsdom` environment.

## Core rules
- **Query by accessibility first:** `getByRole`, `getByLabelText`, `getByText`. Use `getByTestId` only as a last resort.
- **Use `userEvent`, not `fireEvent`**, for interactions (it models real user behavior: focus, key sequences).
- **Async:** use `findBy*` and `waitFor` for state that resolves later; never arbitrary `setTimeout`. Use `await` consistently.
- **Test behavior, not internals:** assert on what's rendered/announced, not component state or prop wiring.
- **Mock at the project boundary:** mock typed `src/ipc.ts` wrappers. Generated
  commands are not configured; mock raw Tauri APIs only while testing a wrapper.
- Each test is isolated: reset call history and changed implementations with
  clear/reset/restore according to how each mock was modified.

## WhisperPilot specifics
- Mock the typed wrappers exported by `src/ipc.ts`, not React internals or raw command strings in components.
- Assert the UI reflects the relevant loading → success → error path for meeting creation, file transcription, settings, or model download.
- Test user-visible flows such as choosing a file, opening/renaming/deleting a meeting, changing a setting, or managing a model; do not introduce chat or assistant-message cases.
- Test the error path: a rejected IPC call surfaces a visible, accessible error instead of failing silently.
- Assert an `aria-live` region only when the touched UI is designed to announce progress, completion, or errors.

## Heuristics per component/behavior
- Renders expected content for given props/state (happy path).
- Responds correctly to user interaction (`userEvent`).
- Handles the async/error state from IPC.
- Edge: empty meeting library, invalid or over-long meeting title, unavailable model, cancelled file dialog, or in-flight transcription state.

## Anti-patterns (findings)
- `getByTestId` where a role/label query works.
- `fireEvent` for what should be `userEvent`.
- Asserting on internal state / implementation instead of rendered output.
- Mocking internals rather than the IPC boundary.
- Missing failure-path test for any IPC-driven behavior.

## Property-based tests (optional fast-check)

For parsing, stripping, and transformation functions, use `fast-check` to verify invariants across random inputs only after the routed task adds that dependency. See `.claude/conventions/testing-taxonomy.md` §Additional Quality Practices for applicability.

```ts
import fc from "fast-check";

it("is idempotent", () => {
  fc.assert(fc.property(fc.string(), (input) => {
    expect(strip(input)).toBe(strip(strip(input)));
  }));
});
```

Derive every invariant from the approved contract. Idempotence, no-throw,
output bounds, and leakage are illustrative candidates, not universal
requirements. Add `fast-check` only through the routed dependency change.

## Accessibility and IPC contracts

Use accessible queries and user interactions in every component test. Add an axe tool only
when the task first configures it. For IPC contracts, test the affected UI behavior with a
typed `src/ipc.ts` mock and test Rust DTO serialization or command behavior at the Rust
boundary. This repository has no generated bindings or binding-generation CI gate.

For typed events, test payload handling, unsubscribe/cleanup,
duplicate-listener prevention, and stale-event behavior when applicable.
