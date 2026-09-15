# Desktop, privacy, and build architecture

Part of the [architecture map](../architecture.md). Covers desktop-window
integration, privacy boundaries, native packaging, and source ownership.

## Floating Bubble Window (ADR-018)

`commands/bubble.rs` coordinates a separate transparent 120-by-120 logical-pixel
webview. Collapse positions and shows the bubble before hiding main. Restore
moves, shows and focuses main before hiding the bubble; any intermediate failure
rolls back to a visible recovery surface. Saved logical coordinates are clamped
against physical monitor work areas on restore, with the primary monitor as the
removed-display fallback. A five-point pointer threshold separates click from
native drag, so the synthetic click after a drag cannot restore main.

The bubble listens to the backend live-capture snapshot and does not own timers
or capture. Its green/listening, yellow/idle and red/actionable-error rings each
also carry a distinct icon and accessible name. Always on Top is persisted and
applied without changing capture; its enabled form is visible on all workspaces.
The transparent implementation enables Tauri `macos-private-api`, so the
supported package is the signed/notarized direct DMG. An App Store distribution
must use the opaque fallback or a separately reviewed native panel.

## Security And Privacy

**Local transcription and MFU detail generation make no network calls** and
there is **no telemetry**. The only networked operations are user-initiated
model downloads, optional app update, and explicitly selected Cloud Meeting.
Cloud Meeting sends live capture audio only to the selected authenticated
provider over TLS; transcripts remain local. Provider keys are stored only in
macOS Keychain, not `.env`, settings, logs, or returned IPC values. Cloud
connection or protocol failures stop capture and cannot silently use Local.
File writes: the temporary ffmpeg WAV (deleted after use), the SQLite library and
non-secret settings store under the app support directory, downloaded model
files, app-owned Recorder CAF artifacts (ADR-017), and user-chosen export
destinations. Recorder audio and transcripts remain local and must not enter
application logs.

## Build MFU

- Tauri v2 + React 19 + TypeScript; Vite dev server on port 1420.
- `whisper-rs = { features = ["metal"] }`; `rusqlite = { features = ["bundled"] }`.
- ffmpeg on PATH. M2 adds sherpa-onnx (via the `sherpa-rs` crate, prebuilt
  binaries fetched at build time); M3 adds llama.cpp — both Metal, local.
- Meeting (WP-70) adds `screencapturekit` for system-audio loopback, a
  macOS-only-target dependency like `whisper-rs`'s `metal` feature.
  ScreenCaptureKit is Apple-only. Building `screencapturekit` needs the full
  Xcode.app installed (not just Command Line Tools) — see Meeting Audio
  Capture above.
- WP-106 adds macOS Keychain credentials plus `tokio-tungstenite` with Rustls
  roots for Cloud BYOK Meeting WebSocket transports.
- M2 adds an HTTP client for SHA-verified model downloads and a settings store;
  front-end gains theming (light/dark/system) and i18n (English default).
- Native dylib packaging (WP-60/WP-86): sherpa-rs-sys and dynamically linked
  llama-cpp place eight runtime libraries in the Cargo profile directory:
  sherpa-onnx, ONNX Runtime, and six llama/ggml dylibs. The linker records them
  as `@rpath/…`. `build.rs` links every binary with
  `@executable_path/../Frameworks` and `@executable_path` rpaths. After Cargo
  compilation and immediately before bundling, Tauri's `beforeBundleCommand`
  runs `scripts/stage-native-dylibs.sh`; the script stages all eight dylibs into
  the generated `src-tauri/frameworks/` for
  `bundle.macOS.frameworks` to copy into `Contents/Frameworks`. Without both, a
  packaged build aborts at dyld before `main` while `cargo run` keeps working,
  because cargo supplies a fallback search path the `.app` never gets.
  `src-tauri/tests/packaging.rs` asserts the config, staging, and rpaths. Tauri's
  local/debug bundle signs each staged dylib and deep signature verification is
  part of validation; release identity signing and Apple notarization remain a
  release operation rather than a build-time guarantee.
- Run: `npm install`, then `npm run tauri:dev`.

## Ownership

| Concern                                                          | Owner                                                                                                    |
| ---------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| Tauri command layer (per-domain) + registration in `run()`       | `src-tauri/src/commands/` + `src-tauri/src/lib.rs`                                                       |
| App state, model cache, running-run slots                        | `src-tauri/src/state.rs`                                                                                 |
| ASR identity, capabilities, compatibility, bundle fingerprint    | `src-tauri/src/asr.rs`                                                                                   |
| IPC event payloads                                               | `src-tauri/src/events.rs`                                                                                |
| Audio normalize + decode                                         | `src-tauri/src/audio.rs`                                                                                 |
| Selected Whisper/Qwen Transcription orchestration and progress   | `src-tauri/src/commands/transcription.rs`, `transcribe.rs`, `qwen_gguf_asr.rs`                            |
| SQLite meeting library                                           | `src-tauri/src/store.rs`                                                                                 |
| Transcription persistence facade / DTOs (legacy Meeting names)   | `src-tauri/src/meetings/` (`mod.rs` facade, `dto.rs` DTOs + coalesce)                                    |
| Error type                                                       | `src-tauri/src/error.rs`                                                                                 |
| Model catalog + download                                         | `src-tauri/src/models/` (`catalog.rs` catalog + state, `download.rs` fetch/verify)                       |
| Speaker diarization                                              | `src-tauri/src/diarize/` (`clustering.rs`, `segmentation.rs`, `speakers.rs`, `pipeline.rs`)              |
| Diarization process isolation                                    | `src-tauri/src/diarize_process/` (`transport.rs`, `worker.rs`, `supervise.rs`)                           |
| Two-pane shell: Transcription library and editors                | `src/`                                                                                                   |
| Meeting capture / decode / persistence / IPC facade              | `src-tauri/src/streaming_audio.rs` / `streaming_session.rs` / `streaming_store.rs` / `streaming.rs`      |
| Recorder capture / audio / persistence / IPC facade              | `src-tauri/src/microphone_audio.rs` / `recorder_audio.rs` / `recorder_store.rs` / `commands/recorder.rs` |
| Cloud provider catalog, Keychain credentials, and command facade | `src-tauri/src/cloud_provider.rs` / `src-tauri/src/commands/settings.rs`                                 |
| Meeting tab                                                      | `src/StreamingView.tsx`                                                                                  |
| Recorder tab and settings                                        | `src/RecorderView.tsx` / `src/RecorderSettingsSection.tsx` / `src/AiModelsSection.tsx`                   |
