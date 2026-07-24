# Rust / Tauri Core Testing — cargo test + tokio

Stack for routed local work: `cargo test --manifest-path src-tauri/Cargo.toml`; use
`#[tokio::test]` for async code. CI separately installs and runs `cargo-nextest` as declared
in `.github/workflows/ci.yml`; it is not a Cargo manifest dependency and local routed
validation does not require it. The current manifest does not include `mockall` or `proptest`;
add either only through a routed dependency change.

## Core rules
- **Async tests** use `#[tokio::test]`; pick `flavor = "multi_thread"` only when the test needs real concurrency.
- **Isolate external effects** with a narrow seam, fixture, temporary directory, or in-memory/temp-file database. Never operate on the user's app-data directory or download a real model in a unit test.
- **Clock/time:** inject or bound time-dependent behavior so tests do not rely on arbitrary wall-clock sleeps.
- **Assert errors by variant** when the public type exposes variants; otherwise
  assert a stable public error contract, not incidental message text.
- **Determinism:** no shared mutable global state across tests; each test sets up its own fixtures and has no ordering dependency.
- Unit tests live in `#[cfg(test)] mod tests` next to the code; cross-module/integration tests live in `tests/`.

## WhisperPilot specifics (transcription, diarization, storage, commands)
- **Audio/transcription/diarization** tests use small checked-in or generated fixtures and cover supported input handling, segmentation/diarization outcome mapping, and failure paths without relying on a user file or model download.
- **Model management** tests cover catalog state, destination selection, checksum/error handling where exposed, and progress-event payload behavior without network access.
- **Storage** tests cover meetings, transcript segments, and settings persistence/readback against a temporary database or temporary app-data directory.
- **Tauri commands** stay thin; test the plain function for behavior and add a
  focused command/IPC contract test when wiring, arguments, injected state,
  error conversion, or serialization changes.

## Heuristics per function/behavior
- Happy path (correct output).
- Boundary/edge inputs (empty/invalid title, empty media result, large transcript, timestamp boundaries).
- Error path (for a changed public Rust function, test every `AppError` variant it constructs
  or propagates; for a changed Tauri command, also test every error outcome explicitly named
  in the routed TaskPilot item's scenarios or DoD).
- Async cancellation or timeout only when the production behavior exposes it; make the test deterministic.

## Anti-patterns (findings)
- Downloading models, opening native dialogs, or hitting the user's filesystem/DB in unit tests.
- Matching errors by message string instead of variant.
- Shared global mutable state, model cache, or test ordering dependence.
- `#[test]` on an async fn (won't compile/await correctly) instead of `#[tokio::test]`.
- Timeout tests that rely on real wall-clock sleeps (flaky).

## Property-based tests (optional)

For parsing and transformation functions, use `proptest` only after adding it as a routed dev dependency. See `.claude/conventions/testing-taxonomy.md` §Additional Quality Practices for applicability.

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

Property tests live inside `proptest! { }` blocks alongside existing `#[cfg(test)]` modules after the dependency is added.

## Contract tests (DTO serialization)

Every affected IPC struct must assert explicit expected JSON keys/values and
have a round-trip test: serialize → deserialize → assert full equality.

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

For workflows that compose storage with transcription or diarization outcome handling, write
integration tests in `src-tauri/tests/` using temporary directories/databases and bounded
fixtures. Integration tests verify the same composition that Tauri commands expose, without
native dialogs, model downloads, or user data.
