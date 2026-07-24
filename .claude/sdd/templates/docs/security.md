# Security

<!-- Optional extension doc. Owns threat model, permissions, and secrets handling.
     WhisperPilot is a local-first, single-user desktop app with no cloud transcription
     or user authn/authz. Its security surface includes local media, transcripts, notes,
     model downloads, Tauri IPC, and macOS distribution controls. Leave a summary + link
     in architecture.md and register this doc in INDEX.md. -->

## Threat Model

<!-- Assets to protect (local media, transcripts, notes, database, model assets), trust
     boundaries (React ↔ Rust IPC and app ↔ model-download hosts), and the main threats
     considered. -->

## Permissions

<!-- Only macOS permissions actually required by implemented behavior, plus verified
     code-signing or entitlement requirements. Do not list live-capture permissions. -->

## Secrets & Sensitive Data

<!-- Where secrets live and how they are supplied, if any. Refer to the root contract for
     secret handling and never instruct readers to inspect secret files. Record which
     layer can access each secret and what sensitive data is never persisted or logged. -->

## Practices

<!-- Input validation at the IPC boundary, HTTPS and SHA verification for model downloads,
     dependency hygiene (npm + cargo), and the no-telemetry/no-remote-processing posture. -->
