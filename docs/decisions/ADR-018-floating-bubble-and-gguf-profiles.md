# ADR-018: Native floating bubble and qualified GGUF profiles

- **Status:** accepted
- **Date:** 2026-09-12
- **Deciders:** Alexey Platkovsky
- **Relates to:** [ADR-006](ADR-006-llamacpp-qwen-summary.md),
  [ADR-014](ADR-014-streaming-mode-coexists-with-batch-meeting.md), and
  [ADR-017](ADR-017-recorder-retains-recoverable-local-audio.md)

## Context

Recorder needs a compact status and restore surface that remains usable while
the main webview is hidden. It must not own capture timing, and a failed window
transition must not leave both surfaces inaccessible. A transparent circular
window is the intended appearance; Tauri exposes this on macOS through its
private-API feature, which is incompatible with Mac App Store distribution.

WhisperPilot also needs a newer text-model lineup without adding a second MLX
runtime. The candidates have different prompt protocols and resource envelopes,
so treating every GGUF as interchangeable would leak reasoning tokens or make
model behavior unpredictable during migration.

## Decision

The bubble is a separate 120 by 120 logical-pixel Tauri webview window. Rust
owns its creation, saved logical position, monitor clamping, show-before-hide
collapse, restore-before-hide rollback, Always on Top, and all-Spaces behavior.
The renderer only presents the backend live-capture snapshot and distinguishes
idle, listening, and actionable error through both ring color and an icon/name.
Native drag begins only after five logical pixels; a drag never dispatches the
restore click. Restore positions the main window at the bubble top-left, clamped
to an available monitor, with the primary work area as stale-display fallback.

WhisperPilot enables Tauri `macos-private-api` for transparent window support.
The supported distribution channel is therefore a signed/notarized direct DMG,
not the Mac App Store. If that channel changes, the fallback is an opaque 120 px
panel with the same interaction and accessibility contract; capture remains
backend-owned in either presentation.

Text inference stays on the existing Metal `llama.cpp` runtime. Fresh installs
recommend Qwen3.5 4B Q4_K_M as the Fast profile. Qwen3.8 9B Q6_K and Gemma 4
12B QAT Q4_0 are explicit Quality choices. Existing model files on disk are
never overwritten or downloaded automatically; the retired Qwen3 4B Q3_K_L
profile is no longer selectable. Each promoted
asset is revision, byte-size, and SHA-256 pinned. Downloads use disk-space
preflight, retained `.part` files and HTTP Range resume, then SHA verification
and atomic rename.

Qwen3.5 and Qwen3.8 use a pinned ChatML no-thinking formatter because the
compact `llama-cpp-2` template API cannot pass their Jinja reasoning arguments.
Gemma 4 uses its pinned canonical no-thinking `<|turn>`/channel protocol from
the GGUF metadata. Visible output additionally strips reasoning/control syntax,
MFU stops at the first complete JSON object, and persisted results keep the
existing schema. Recorder polishing is review-first: raw timestamped segments
are immutable, and accept/revert stores or removes a separate polished view.

## Consequences

- Hiding either renderer cannot pause capture or transcription; Rust remains
  the lifecycle authority.
- Transparent bubble packaging must continue to be tested as a direct-DMG app.
- Mixed-scale, removed-monitor, Spaces, fullscreen, Stage Manager, accessibility,
  and sleep/wake behavior need real-macOS evidence in addition to unit tests.
- One bounded local-LLM scheduler serializes all profiles; profile switching and
  deletion keep the existing mutation barrier and cache invalidation semantics.
- Model-specific prompt formatting is a compatibility boundary. A newly listed
  GGUF must pass the product corpus and reasoning-leak checks before promotion.
- Quality profiles require materially more unified memory and remain opt-in.

## Alternatives Considered

- **An in-main-window overlay** — rejected because it disappears with the main
  webview and cannot be positioned independently above other applications.
- **An AppKit panel plugin** — deferred because Tauri supplies the required
  behavior with less native surface area for the current direct-DMG channel.
- **An opaque window only** — retained as the distribution fallback, but not the
  preferred direct-DMG appearance.
- **Move all models to MLX** — rejected for this phase because it would add a
  second inference stack and invalidate the current scheduler/download/runtime
  behavior without evidence that all supported workflows improve.
- **One universal embedded chat template** — rejected after real Metal testing:
  Qwen exposed reasoning and Gemma's canonical Jinja exceeded the compact C
  template renderer. Pinned no-thinking formatters are deterministic and tested.
