# Component Structure & React Performance

## Structure
- One component per file; co-locate its styles and tests.
- Keep business logic in plain TS modules (pure functions) so it is testable without rendering.
- Extract large component bodies into smaller subcomponents early — improves both readability and memoization boundaries.

## Re-render hygiene
- Keep state subscriptions and contexts narrow; avoid placing unrelated application state in one provider.
- Memoize expensive derived values with `useMemo`; memoize callbacks passed to memoized children with `useCallback`.
- Wrap pure presentational children in `React.memo` when they re-render under an often-changing parent.
- Stable `key`s for lists; never array index for dynamic/reorderable lists.

## Transcript and meeting-specific (WhisperPilot)
- Transcript and meeting lists can grow long. Virtualize only after profiling or a reproducible interaction problem; mark this as `[opt]` until then.
- Keep a transcription progress update or model-download event from needlessly re-rendering unrelated settings, meeting, or transcript UI.
- Preserve the user's selected meeting and reading position when a refresh or background model-status update completes, unless the user initiated navigation.

## Notes
- React 19: the compiler may auto-memoize, but do not rely on it for correctness; explicit stable identities still matter for lists and effects.
- Flag missing/incorrect `useEffect` dependency arrays — stale closures are correctness bugs, not just perf.
- Performance suggestions are `[opt]` and never block completion unless they cause a visible hang.
