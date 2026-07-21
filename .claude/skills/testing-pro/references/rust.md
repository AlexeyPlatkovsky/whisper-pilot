# Rust / Tauri Core Testing — cargo-nextest + tokio + mockall

Stack: **cargo-nextest** (preferred runner; process-per-test isolation, faster), `#[tokio::test]` for async, **`mockall`** for trait-based mocking. `cargo test` remains the fallback (and is required for doctests, which nextest doesn't run).

## Core rules
- **Async tests** use `#[tokio::test]`; pick `flavor = "multi_thread"` only when the test needs real concurrency.
- **Isolate external effects behind traits**, then mock with `mockall`:
  - Subprocess execution (running a CLI) → a `CommandRunner`/`ProcessRunner` trait, mocked in tests; never spawn the real `codex`/`claude`/`gemini` in unit tests.
  - Clock/time (timeouts) → an injectable time source so timeout tests are deterministic.
  - SQLite/storage → test against an in-memory or temp-file database, not the user's real store.
- **Assert errors by variant**, not by string: `assert!(matches!(err, AdapterError::TimedOut))`, not substring matching on a message.
- **Determinism / nextest-friendliness:** no shared mutable global state across tests; each test sets up its own fixtures. Avoid ordering dependencies (nextest runs tests in separate processes).
- Unit tests live in `#[cfg(test)] mod tests` next to the code; cross-module/integration tests live in `tests/`.

## WhisperPilot specifics (adapters, routing, storage, commands)
- **CLI adapter** tests cover: command/argument construction (correct binary + flags like `codex exec --json -s read-only`), parsing of the structured output into the typed result, mapping of failures to the `AdapterError` taxonomy (binaryNotFound / notAuthenticated / nonZeroExit / timedOut / outputParseFailure / cancelled), timeout behavior, and cancellation — all with a mocked runner.
- **Routing layer** tests cover: a request routes to the correct adapter; unknown/!registered targets error cleanly.
- **Storage** tests cover: messages persist and read back; session references (`codex_session_id`) round-trip — against a temp DB.
- **Tauri commands** stay thin; test the underlying plain function, not the `#[tauri::command]` wrapper.

## Heuristics per function/behavior
- Happy path (correct output).
- Boundary/edge inputs (empty prompt, large output).
- Error path (each mapped `AdapterError` variant has a test).
- Async cancellation / timeout (deterministic via injected time + mocked runner).

## Anti-patterns (findings)
- Spawning real CLI binaries or hitting the real filesystem/DB in unit tests.
- Matching errors by message string instead of variant.
- Shared global mutable state or test ordering dependence.
- `#[test]` on an async fn (won't compile/await correctly) instead of `#[tokio::test]`.
- Timeout tests that rely on real wall-clock sleeps (flaky).

## Property-based tests (proptest)

For parsing and stripping functions, use `proptest` to verify invariants. See `.claude/conventions/testing-taxonomy.md` §Additional Quality Practices for requirements.

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn strip_never_panics(s in "\\PC*") {
        let _ = strip_ansi(&s);
    }

    #[test]
    fn strip_is_idempotent(s in "\\PC*") {
        let first = strip_ansi(&s);
        assert_eq!(strip_ansi(&first), first);
    }
}
```

Add `proptest = "1"` to `[dev-dependencies]`. Property tests live inside `proptest! { }` blocks alongside existing `#[cfg(test)]` modules.

## Contract tests (round-trip serde)

Every IPC struct must have a round-trip test: serialize → deserialize → assert equality.

```rust
#[test]
fn adapter_request_round_trips() {
    let original = AdapterRequest { /* ... */ };
    let json = serde_json::to_value(&original).unwrap();
    let round_tripped: AdapterRequest = serde_json::from_value(json).unwrap();
    assert_eq!(round_tripped.assistant, original.assistant);
}
```

Contract tests live in the same `#[cfg(test)]` module as the struct definition.

## Integration tests

For pipelines that compose store + adapter registry + routing, write integration tests in `src-tauri/tests/` using an in-memory SQLite store and stub adapters (no real CLI spawns):

```rust
use side_pilot_lib::storage::Store;
use side_pilot_lib::adapters::AdapterRegistry;

#[tokio::test]
async fn full_roundtrip() {
    let store = Store::in_memory().unwrap();
    let registry = make_stub_registry();
    // exercise the pipeline
}
```

Integration tests verify the same composition the Tauri commands use at runtime.
