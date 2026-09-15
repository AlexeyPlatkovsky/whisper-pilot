# Development

Developer guide for building and running WhisperPilot from source. Product
scope lives in [`idea.md`](idea.md); technical architecture in
[`architecture.md`](architecture.md); the instruction contract for AI agents
working on this repo is [`../AGENTS.md`](../AGENTS.md).

## Prerequisites

- macOS on Apple Silicon (macOS 13+)
- Rust (stable), Node.js 20.19+ or 22.12+
- `ffmpeg` on PATH (`brew install ffmpeg`)
- libclang (bundled with Xcode Command Line Tools; already required for other
  native deps) — `sherpa-rs`'s build script always runs `bindgen`, download or
  not
- Network access at `cargo build` time: `sherpa-rs`'s `download-binaries`
  feature fetches prebuilt sherpa-onnx shared libraries for the host platform
  during the build, not just at first run

## Setup

```sh
npm install
npm run tauri:dev      # launches the app (compiles whisper.cpp with Metal on first run)
```

Vite serves the front end on port 1420; Tauri drives the Rust core in
`src-tauri/`.

`npx tauri build` produces `WhisperPilot.app` and a DMG under
`src-tauri/target/release/bundle/`. The build also generates
`src-tauri/frameworks/` — a staging copy of the sherpa-onnx and ONNX Runtime
dylibs that the bundler puts into `Contents/Frameworks`. It is generated and
gitignored; do not edit or commit it. See
[`architecture/desktop.md`](architecture/desktop.md#build-mfu) for why it exists.

## Scripts

| Command | Purpose |
|---|---|
| `npm run tauri:dev` | Run the app in development |
| `npm run build` | Type-check and build the front end |
| `npm test` / `npm run test:run` | Front-end tests (Vitest) |
| `npm run test:api` | Rust core tests (`cargo test`) |
| `npm run test:coverage` | Front-end tests with the enforced coverage thresholds |
| `npm run typecheck` | TypeScript type-check only |
| `npm run lint` | ESLint over `src/` |
| `npm run lint:ui` | Enforce shared confirmation and destructive-action UI contracts |
| `npm run lint:comments` | Reject oversized explanatory comment blocks |
| `npm run format` / `npm run format:check` | Write or verify Prettier formatting in `src/` |
| `npm run lint:ai-instructions` / `npm run test:ai-instructions` | Validate active agent/skill contracts, model-effort compatibility, TOML syntax, and validator regressions |
| `npm run lint:source-size` / `npm run test:source-size` | Reject production modules at 750 lines, test modules at 1,750, and documentation over 600; also covers scripts, workflows, and `build.rs` |
| `npm run test:repository-policy` | Test staged version and pre-commit enforcement |
| `npm run version:check` | Verify all release-version records and the README badge agree |
| `npm run version:minor` / `npm run commit:minor` | Small fix or local change: increment SemVer PATCH |
| `npm run version:feature` (`version:major`) / `npm run commit:feature` (`commit:major`) | Feature or medium change: increment SemVer MINOR |
| `WHISPERPILOT_RELEASE_AUTHORIZED=1 npm run commit:release -- -m "…"` | Increment SemVer MAJOR; only after an explicit user release request |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | Verify Rust formatting |
| `cargo build --manifest-path src-tauri/Cargo.toml --all-targets --all-features` | Build every Rust target and feature |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings` | Reject Rust warnings and lints |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Run the default Rust suite |

## Debugging

The agent classifies a task as small or feature-sized; the hook validates the chosen SemVer transition, not diff size.
If a commit attempt fails, rerun the same `commit:*` command: it reuses the pending bump. Add `--amend --no-edit` to
amend the current task commit without incrementing its version again.

Run with `RUST_LOG=info npm run tauri:dev` and capture stderr before drawing
conclusions from behavior alone. Transcription decodes are logged at debug
level.

## Project Layout

```
React UI (src/)  ──Tauri IPC──▶  Rust core (src-tauri/src/)
  Transcriptions list              lib.rs        crate root; `run()` registration
  Transcription workspace          commands/     thin Tauri command layer
  transcript editor                audio.rs      ffmpeg normalize + direct PCM decode
  ...                               store.rs      SQLite Transcription library
```

Start with [`architecture.md`](architecture.md) for the layer map, then open
the linked functional document for the relevant IPC contract, data model, or
build boundary.

## Environment Variables

- `WHISPERPILOT_MODEL_PATH` — override the Whisper model path.
- `WHISPERPILOT_TEST_AUDIO` — WAV fixture path for the ignored model-backed
  tests (e.g. `cargo test --test wp84_callback_regression -- --ignored`).

## Testing Strategy

See [`testing.md`](testing.md) for test levels and quality gates.
