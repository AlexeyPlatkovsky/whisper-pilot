# Documentation Index

<!-- Live map of the docs tree and feature registry. Lookup aid only:
     no routing, gates, or behavioral rules. Keep in sync after any doc/feature change
     (the sdd-index-sync skill rebuilds this file). -->

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

<!-- Add a row for each optional extension doc that exists, e.g.: -->
<!-- | `design-book.md` | UI design contract: tokens, themes, patterns | You do UI work | -->
<!-- | `api.md` | Tauri IPC contracts (commands, events) | You need the IPC surface | -->
<!-- | `db.md`  | SQLite data model and schema | You need entities or migrations | -->

## Feature Registry

| ID | Feature | Requirements | Tasks | Scenarios | Serves |
| --- | --- | --- | --- | --- | --- |
<!-- The index synchronizer adds one row per present feature. TaskPilot owns work status. -->

## Decision Log

| ADR | Title | Status |
| --- | --- | --- |
<!-- The index synchronizer adds one row per present ADR. -->
