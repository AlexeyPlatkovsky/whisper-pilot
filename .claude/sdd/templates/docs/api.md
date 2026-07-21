# API (Tauri IPC Contract)

<!-- Optional extension doc. Owns interface contracts split out of architecture.md.
     In WhisperPilot the API surface is the Tauri v2 IPC boundary: invoke commands and
     emitted events between the React front-end (src/ipc/commands.ts) and the Rust core
     (src-tauri/src/commands.rs). Leave a summary + link in architecture.md. Register
     this doc in INDEX.md. When the contract grows into several areas, promote to
     docs/api/ with a mini-index. -->

## Overview

<!-- The IPC surface: command groups (session, capture, engines, models), event channels,
     and where the TS and Rust sides of the contract live. -->

## Conventions

<!-- Shared rules: serde type-shape parity (TS ↔ Rust), error propagation
     (WhisperPilotError / AppError), event naming, payload size limits, what never crosses
     the boundary (e.g. API keys stay in Rust). -->

## Commands

### <command_name>

- **Purpose:** <what it does>
- **Args:** <parameters / payload schema>
- **Returns:** <result schema>
- **Errors:** <notable error cases>
- **Events emitted:** <event name + payload, or none>

## Events

### <event-name>

- **Emitted when:** <trigger>
- **Payload:** <schema>
- **Consumed by:** <front-end hook / store>
