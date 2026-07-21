# Front-end Testing — Vitest + React Testing Library

Stack: **Vitest** (runner, Vite-native, fast), **React Testing Library** (RTL), `@testing-library/jest-dom`, `@testing-library/user-event`, `jsdom` environment.

## Core rules
- **Query by accessibility first:** `getByRole`, `getByLabelText`, `getByText`. Use `getByTestId` only as a last resort.
- **Use `userEvent`, not `fireEvent`**, for interactions (it models real user behavior: focus, key sequences).
- **Async:** use `findBy*` and `waitFor` for state that resolves later; never arbitrary `setTimeout`. Use `await` consistently.
- **Test behavior, not internals:** assert on what's rendered/announced, not component state or prop wiring.
- **Mock judiciously — at the boundary:** mock the Tauri IPC layer (`invoke` / generated `commands`) with `vi.mock`, not internal helpers. Don't mock React.
- Each test is isolated: no shared mutable state between tests; reset mocks in `afterEach` (or `clearMocks: true`).

## WhisperPilot specifics
- Wrap components under test in their real providers (TanStack Query client, Zustand store) using a small `renderWithProviders` helper.
- For IPC, mock the generated command/`invoke` to return typed fixtures; assert the UI reflects loading → success → error.
- Test the send flow: typing a prompt + pressing Enter triggers the mutation; the response appears; the input clears.
- Test the error path: a rejected IPC call surfaces a visible, accessible error (not a silent failure).
- Assert the `aria-live` region receives the new assistant message.

## Heuristics per component/behavior
- Renders expected content for given props/state (happy path).
- Responds correctly to user interaction (`userEvent`).
- Handles the async/error state from IPC.
- Edge: empty history, very long message, in-flight (disabled send) state.

## Anti-patterns (findings)
- `getByTestId` where a role/label query works.
- `fireEvent` for what should be `userEvent`.
- Asserting on internal state / implementation instead of rendered output.
- Mocking internals rather than the IPC boundary.
- Missing failure-path test for any IPC-driven behavior.

## Property-based tests (fast-check)

For parsing, stripping, and transformation functions, use `fast-check` to verify invariants across random inputs. See `.claude/conventions/testing-taxonomy.md` §Additional Quality Practices for requirements.

```ts
import fc from "fast-check";

it("is idempotent", () => {
  fc.assert(fc.property(fc.string(), (input) => {
    expect(strip(input)).toBe(strip(strip(input)));
  }));
});
```

Key invariants to verify: idempotence, no throw on any input, output is never longer than input (for reduction functions), no secrets/URLs leaked in output. Property tests live in `*.proptest.ts` files alongside regular tests.

## Accessibility tests (jest-axe)

Run an axe audit after rendering a component with live data. Use the `src/test/a11y.ts` helper.

```ts
import { checkA11y } from "../test/a11y";

it("has no a11y violations", async () => {
  const { container } = render(<MyComponent />);
  await checkA11y(container, "my component expanded");
});
```

A11y tests live in `*.a11y.test.tsx` files. E2E a11y uses `@axe-core/playwright`.

## Contract tests (IPC shapes)

Verify generated TypeScript types match the Rust source and are importable. The CI gate `cargo test export_bindings` + `git diff --exit-code src/chat/generated` ensures generated files are never stale.

```ts
it("all generated contract modules are importable", async () => {
  for (const mod of ["AdapterRequest", "Message", "Route" /* ...more modules */]) {
    await expect(import(`./generated/${mod as string}`)).resolves.toBeDefined();
  }
});
```

Contract tests live in `src/chat/contract.test.ts`.
