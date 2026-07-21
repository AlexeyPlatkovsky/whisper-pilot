# Security

<!-- Optional extension doc. Owns threat model, permissions, secrets handling.
     WhisperPilot is a local-first single-user desktop app: there is no user authn/authz;
     the security surface is macOS permissions, the OpenAI API key, and privacy of
     captured audio/transcripts. Leave a summary + link in architecture.md.
     Register this doc in INDEX.md. -->

## Threat Model

<!-- Assets to protect (API key, transcripts, session data), trust boundaries
     (React ↔ Rust IPC, app ↔ OpenAI cloud), and the main threats considered. -->

## Permissions

<!-- macOS permissions the app requires: Screen Recording for ScreenCaptureKit audio
     capture; code signing / entitlements needed to obtain them. -->

## Secrets & Sensitive Data

<!-- Where secrets live and how they are supplied. Refer to the root contract for secret
     handling; never instruct readers to inspect secret files. Record which layer can access
     each secret and what sensitive data is never persisted or logged. -->

## Practices

<!-- Input validation at the IPC boundary, transport security for OpenAI calls,
     dependency hygiene (npm + cargo), no telemetry or remote crash reporting. -->
