# WhisperPilot

Offline macOS app that transcribes local **audio and video files** into
accurate, speaker-attributed transcripts (Russian first, English later) and
generates a short summary — fully on-device, no live capture, no cloud.

See [docs/idea.md](docs/idea.md) for scope and milestones, and
[docs/architecture.md](docs/architecture.md) for the technical design.

## Status

- **M1 (done):** Add file → extract audio (ffmpeg) → Whisper `large-v3-turbo` on
  Metal, full-file Russian transcription → editable transcript → save.
- **M2 (planned):** Speaker separation ("by roles") via sherpa-onnx.
- **M3 (planned):** Local-LLM summary/MFU via llama.cpp (Qwen2.5).

## Prerequisites

- macOS on Apple Silicon (macOS 13+)
- Rust (stable), Node.js 20+
- `ffmpeg` on PATH (`brew install ffmpeg`)
- A Whisper GGML model. M1 reuses an existing `large-v3-turbo` artifact; override
  the path with `MFUPILOT_MODEL_PATH=/path/to/ggml-large-v3-turbo-q8_0.bin`.

## Develop

```sh
npm install
npm run tauri:dev      # launches the app (compiles whisper.cpp with Metal on first run)
```

## Test

```sh
npm run test:api       # Rust tests
# End-to-end pipeline check (needs a model + ffmpeg):
cargo test --manifest-path src-tauri/Cargo.toml --test pipeline -- --ignored --nocapture
```

## Layout

```
src/                 React UI (App.tsx, ipc.ts, styles.css)
src-tauri/src/       Rust core (lib.rs, audio.rs, transcribe.rs, error.rs)
docs/                idea.md, architecture.md
.claude/             AI instruction system (skills, agents, pipelines, conventions)
```
