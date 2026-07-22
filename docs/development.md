# Development

Developer guide for building and running WhisperPilot from source. Product
scope lives in [`idea.md`](idea.md); technical architecture in
[`architecture.md`](architecture.md); the instruction contract for AI agents
working on this repo is [`../AGENTS.md`](../AGENTS.md).

## Prerequisites

- macOS on Apple Silicon (macOS 13+)
- Rust (stable), Node.js 20+
- `ffmpeg` on PATH (`brew install ffmpeg`)

## Setup

```sh
npm install
npm run tauri:dev      # launches the app (compiles whisper.cpp with Metal on first run)
```

Vite serves the front end on port 1420; Tauri drives the Rust core in
`src-tauri/`.

## Scripts

| Command | Purpose |
|---|---|
| `npm run tauri:dev` | Run the app in development |
| `npm run build` | Type-check and build the front end |
| `npm test` / `npm run test:run` | Front-end tests (Vitest) |
| `npm run test:api` | Rust core tests (`cargo test`) |
| `npm run typecheck` | TypeScript type-check only |
| `npm run lint` | ESLint over `src/` |
| `npm run format` | Prettier over `src/` |

## Debugging

Run with `RUST_LOG=info npm run tauri:dev` and capture stderr before drawing
conclusions from behavior alone. Transcription decodes are logged at debug
level.

## Project Layout

```
React UI (src/)  ──Tauri IPC──▶  Rust core (src-tauri/src/)
  meetings list                    lib.rs        command + event registration, AppState
  meeting workspace                audio.rs      ffmpeg normalize + WAV decode
  transcript editor                transcribe.rs whisper (Metal) full-file decode, progress
  ...                               store.rs      SQLite meeting library
```

See [`architecture.md`](architecture.md) for the full layer map, IPC contract,
data model, and build notes.

## Environment Variables

- `WHISPERPILOT_MODEL_PATH` — override the Whisper model path.

## Testing Strategy

See [`testing.md`](testing.md) for test levels and quality gates.
