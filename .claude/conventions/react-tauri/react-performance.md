# Component Structure & React Performance

## Structure
- One component per file; co-locate its styles and tests.
- Keep business logic in plain TS modules (pure functions) so it is testable without rendering.
- Extract large component bodies into smaller subcomponents early — improves both readability and memoization boundaries.

## Re-render hygiene
- Select narrow Zustand slices; avoid subscribing to the whole store.
- Memoize expensive derived values with `useMemo`; memoize callbacks passed to memoized children with `useCallback`.
- Wrap pure presentational children in `React.memo` when they re-render under an often-changing parent.
- Stable `key`s for lists; never array index for dynamic/reorderable lists.

## Chat-specific (WhisperPilot)
- The message list can grow long — virtualize once it's large (`[opt]` until it's a real problem).
- A streaming/blocking response should update without re-rendering the entire transcript: isolate the in-flight message into its own component.
- Auto-scroll-to-bottom must not fight the user scrolling up to read history.

## Notes
- React 19: the compiler may auto-memoize, but do not rely on it for correctness; explicit stable identities still matter for lists and effects.
- Flag missing/incorrect `useEffect` dependency arrays — stale closures are correctness bugs, not just perf.
- Performance suggestions are `[opt]` and never block completion unless they cause a visible hang.
