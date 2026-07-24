# Operations

<!-- Optional extension doc. Owns runtime, build/signing, observability, and runbooks.
     WhisperPilot is a local macOS desktop app with no server deployment. Document only
     verified build, signing, permission, and logging behavior; do not introduce live
     capture or Screen Recording requirements. Leave a summary + link in
     architecture.md and register this doc in INDEX.md. -->

## Environments

<!-- How development and release builds differ, using commands verified in
     package.json and Tauri configuration. Record required signing or macOS permissions
     only when they are actually configured. -->

## Build & Signing

<!-- How a runnable app is produced, including only verified Tauri build commands,
     signing identities, entitlements, notarization, and release artifacts. -->

## Configuration

<!-- Required configuration and how it is supplied at runtime. Refer to the root contract
     and durable architecture documentation; never instruct readers to inspect secret files. -->

## Observability

<!-- Verified log destinations, levels, and collection steps. State which sensitive local
     data (audio, transcript text, notes, file paths) must never be logged. Do not claim
     a log path, telemetry, or remote reporting mechanism without implementation evidence. -->

## Runbooks

<!-- Step-by-step responses to common operational situations
     (for example, missing local model assets, ffmpeg unavailable, or local-log
     collection for a bug report). -->

### <situation>

1. <step>
