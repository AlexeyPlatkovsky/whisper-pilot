# Operations

<!-- Optional extension doc. Owns runtime, build/signing, observability, runbooks.
     WhisperPilot is a local macOS desktop app — there is no server deployment; "operations"
     means dev/release builds, code signing, macOS permissions, and local logs.
     Leave a summary + link in architecture.md. Register this doc in INDEX.md. -->

## Environments

<!-- How builds differ: `npm run tauri:dev` dev build vs signed debug/release .app bundle
     (`npm run tauri build -- --debug`), and what each requires (signing identity,
     Screen Recording permission). -->

## Build & Signing

<!-- How a runnable app is produced: Tauri build commands, TAURI_SIGNING_IDENTITY,
     Entitlements.plist, granting Screen Recording permission. -->

## Configuration

<!-- Required configuration and how it is supplied at runtime. Refer to the root contract
     and durable architecture documentation; never instruct readers to inspect secret files. -->

## Observability

<!-- Logs at ~/Library/Logs/WhisperPilot/voicepilot-YYYY-MM-DD.log (and stderr); log levels
     per build type; RUST_LOG override; what logs never contain (audio, transcript text,
     API keys). -->

## Runbooks

<!-- Step-by-step responses to common operational situations
     (e.g. capture permission denied, missing model file, log collection for a bug report). -->

### <situation>

1. <step>
