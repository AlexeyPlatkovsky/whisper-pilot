# Architecture

<!-- Owns: technical structure. For WhisperPilot, describe the React/Tauri/Rust
     boundaries and local-file processing without restating product flows. Keep
     product/UX in design.md and decision rationale in decisions/. Link to ADRs for
     the "why" behind a choice, and clearly label planned components as planned. -->

## System Context

<!-- What the system is, its boundaries, and the external actors/systems it talks to. -->

## Components

<!-- The major parts and their responsibilities. A diagram or table is welcome. -->

| Component | Responsibility | Notes |
| --- | --- | --- |
| <name> | <what it does> | <tech / location> |

## Data Model

<!-- Key entities, their relationships, and where state lives. -->

## Tech Stack

<!-- Languages, frameworks, datastores, infra. Note versions where they matter. -->

## Integrations

<!-- External dependencies and how the system depends on them. WhisperPilot has no
     cloud processing; user-initiated model downloads are the only normal network
     operation. -->

## Constraints

<!-- Performance, privacy, platform, and dependency constraints. Include the
     offline/local-first guarantee, macOS scope, and any required local tools. -->

## Cross-Cutting Concerns

<!-- Auth, logging, error handling, config, observability, i18n. -->

## Key Decisions

<!-- Link to the ADRs that shaped this architecture. -->

- See `decisions/ADR-<NNN>-<slug>.md` — <one-line summary>
