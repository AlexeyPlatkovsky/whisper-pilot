---
name: taskpilot-work
description: Inspect or update WhisperPilot TaskPilot records and lifecycle state.
---

# TaskPilot work

## Apply when

Use for TaskPilot administration or when non-trivial tracked work needs its record inspected or updated. Do not use when
the user explicitly says not to use TaskPilot or for TaskPilot-exempt AI-landscape work.

## Invariants

- Project key is `WP`.
- Inspect the item and parent context before any mutation.
- Ask before creating an item.
- Work starts from `ready`, or from `blocked` after an explicit resume.
- Discovery may move `backlog → in_progress → ready`.
- Delivery may move `ready → in_progress → done`.
- Mark `done` only after the item's DoD, validation, review, and documentation decision pass.
- Validate and reload after every mutation; report the actual state after partial failure.
- Never store decision history in the description. Put durable decisions in an ADR and local evidence in comments.
- Keep parent, child, blocker, and relation data in their fields rather than duplicating them in prose.

## Record quality

An implementation-ready record states the goal, scope boundary, observable behavior, failure behavior, constraints,
affected surfaces, dependencies, and objective DoD. Unresolved requirements keep the item unready.

## Commands

- List: `taskpilot --json item list`
- Inspect: `taskpilot item show <ID>`
- Create: `taskpilot item create --title "<title>" --type <type> --status backlog`
- Update: `taskpilot item update <ID> --status <status>`
- Link: `taskpilot item parent|blocks|relates <source> <target>`
- Comment: `taskpilot item comment <ID> "<evidence>"`
- Validate: `taskpilot validate`

## Result

Report the operation, item IDs, before and after state, validation result, and any required recovery.
