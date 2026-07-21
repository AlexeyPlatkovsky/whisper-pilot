# Database

<!-- Optional extension doc. Owns persistence: data model, schema, migrations.
     In WhisperPilot persistence is local SQLite managed by the session layer
     (src-tauri/src/session/ — SessionManager, schema + migrations).
     Leave a summary + link in architecture.md. Register this doc in INDEX.md. -->

## Storage

<!-- Engine and location, e.g. SQLite under ~/Library/Application Support/WhisperPilot/;
     key configuration; what is never persisted (raw audio). -->

## Entities

| Entity | Purpose | Key fields | Relationships |
| --- | --- | --- | --- |
| <name> | <what it represents> | <fields> | <links to other entities> |

## Schema Notes

<!-- Indexes, constraints, derived/computed data, retention. -->

## Migrations

<!-- How schema changes are managed, tooling, and ordering rules. -->
