# Documentation Index

<!-- Live map of the authoritative docs tree and feature registry. This is a
     lookup aid only: no routing, gates, or behavioral rules. Keep it in sync
     after every docs or feature change with the sdd-index-sync skill. Include
     only documents and features that actually exist. -->

<!-- TEMPLATE GUIDANCE — delete every comment block opening with this token as
     the final step of creating the index. Keep the opening description above,
     and keep every begin/end marker comment, each table's header row, and
     nothing but the table inside a marker pair: all three are load-bearing, and
     the sync cannot run in update mode without them, even around a table that
     has no data rows yet.

     The three tables below sit inside those markers. The sync owns which rows
     exist (keys: filename, feature ID, ADR ID) and re-derives the key cell plus
     the feature ID-range cells; every other cell is curated and is carried
     across a rebuild byte-identically. Anything outside the markers is never
     touched. Full contract: .claude/skills/sdd-index-sync/SKILL.md
     §Generated Markers And The Key-Preserving Row Merge.

     Do not rename, duplicate, nest, or quote a marker literal anywhere else in
     this file. -->

**Tier:** <Lean | Standard | Full>
**Docs root:** `docs/`

## Documents

<!-- TEMPLATE GUIDANCE — Seed rows: default curated text for the Standard-tier main docs. Edit or
     delete them at creation — the sync drops any whose file does not exist, and
     freezes the rest as that row's curated cells. Add a hand-written row INSIDE
     the markers; a row outside them is invisible to the merge and will be
     duplicated. Extension docs that exist (e.g. `api.md` — Tauri IPC contracts;
     `db.md` — SQLite data model) are appended by the sync with `TODO` cells for
     you to fill in. -->
<!-- sdd-index-sync:begin documents -->
| Document | Owns | Read when |
| --- | --- | --- |
| `idea.md` | Problem, users, scope, principles | You need project intent or scope boundaries |
| `architecture.md` | Technical structure | You need components, data, stack, constraints |
| `design.md` | Product/UX design | You need flows, screens, states |
| `testing.md` | Test strategy | You need how quality is verified |
| `roadmap.md` | Phases and sequencing | You need release plan or priorities |
| `decisions/` | Architectural decisions | You need the rationale behind a choice |
<!-- sdd-index-sync:end documents -->

## Feature Registry

<!-- sdd-index-sync:begin features -->
| ID | Feature | Requirements | Tasks | Scenarios | Serves |
| --- | --- | --- | --- | --- | --- |
<!-- sdd-index-sync:end features -->

<!-- TEMPLATE GUIDANCE — The sync adds one row per present feature, keyed by feature ID. TaskPilot
     owns work status; do not add a status column here. -->

## Decision Log

<!-- sdd-index-sync:begin decisions -->
| ADR | Title | Status |
| --- | --- | --- |
<!-- sdd-index-sync:end decisions -->

<!-- TEMPLATE GUIDANCE — The sync adds one row per present ADR, keyed by ADR ID. -->
