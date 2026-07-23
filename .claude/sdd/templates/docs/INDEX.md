# Documentation Index

<!-- Live map of the authoritative docs tree and feature registry. This is a
     lookup aid only: no routing, gates, or behavioral rules. Keep it in sync
     after every docs or feature change with the sdd-index-sync skill. Include
     only documents and features that actually exist. -->

**Tier:** <Lean | Standard | Full>
**Docs root:** `docs/`

## Documents

| Document | Owns | Read when |
| --- | --- | --- |
| `idea.md` | Problem, users, scope, principles | You need project intent or scope boundaries |
| `architecture.md` | Technical structure | You need components, data, stack, constraints |
| `design.md` | Product/UX design | You need flows, screens, states |
| `testing.md` | Test strategy | You need how quality is verified |
| `roadmap.md` | Phases and sequencing | You need release plan or priorities |
| `decisions/` | Architectural decisions | You need the rationale behind a choice |

<!-- Add a row for each optional extension doc that actually exists, e.g.: -->
<!-- | `design-book.md` | UI design contract: tokens, themes, patterns | You do UI work | -->
<!-- | `api.md` | Tauri IPC contracts (commands, events) | You need the IPC surface | -->
<!-- | `db.md`  | SQLite data model and schema | You need entities or migrations | -->

## Feature Registry

| ID | Feature | Requirements | Tasks | Scenarios | Serves |
| --- | --- | --- | --- | --- | --- |
<!-- The index synchronizer adds one row per present feature. TaskPilot owns
     work status; do not add a status column here. -->

## Decision Log

| ADR | Title | Status |
| --- | --- | --- |
<!-- The index synchronizer adds one row per present ADR. -->
