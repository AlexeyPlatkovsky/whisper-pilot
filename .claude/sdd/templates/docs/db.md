# Database

<!-- Optional extension doc. Owns persistence: data model, schema, migrations.
     In WhisperPilot the local SQLite meeting library is implemented in
     src-tauri/src/store.rs; settings and model assets have their own local storage.
     Record only verified locations and current migration behavior. Leave a summary +
     link in architecture.md and register this document in INDEX.md. -->

## Storage

<!-- Engine and verified storage location; key configuration; retained data; and data
     deliberately not persisted. Do not assume the source media is copied: meetings
     may instead reference the user-selected local file path. -->

## Entities

| Entity | Purpose | Key fields | Relationships |
| --- | --- | --- | --- |
| <name> | <what it represents> | <fields> | <links to other entities> |

## Schema Notes

<!-- Indexes, constraints, derived/computed data, retention. -->

## Migrations

<!-- How schema changes are managed, tooling, and ordering rules. -->
