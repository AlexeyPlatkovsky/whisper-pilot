# API (Tauri IPC Contract)

<!-- Optional extension doc. Owns interface contracts split out of architecture.md.
     In WhisperPilot the API surface is the Tauri v2 IPC boundary between the React
     front end (currently src/ipc.ts) and registered Rust commands (currently
     src-tauri/src/lib.rs). Include only implemented contracts or explicitly labelled
     planned contracts. Leave a summary + link in architecture.md and register this
     document in INDEX.md. Promote to docs/api/ with a mini-index only when it grows
     into independently maintained areas. -->

## Overview

<!-- The IPC surface: command groups, event channels, and the current TypeScript and
     Rust contract locations. WhisperPilot processes user-selected local audio/video
     files; it has no live-capture API. -->

## Conventions

<!-- Shared rules: TS ↔ Rust type-shape parity, error serialization, event naming,
     payload-size limits, and data that must not cross the boundary. Do not invent
     secret-bearing API flows: transcription and notes are local-only. -->

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
