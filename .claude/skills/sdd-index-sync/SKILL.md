---
name: sdd-index-sync
description: Rebuilds docs/INDEX.md (document map, feature registry, decision log) from the current docs tree so the index reflects the files that actually exist. Use after any doc, feature, or ADR change.
---

## Scope

- Regenerate `docs/INDEX.md` only, from the present state of the docs tree.
- Register the main and extension docs that exist, the feature folders with their counts,
  and the ADRs with their status.
- Do not edit any document other than `INDEX.md`, invent statuses, or add routing, gates,
  or behavioral rules to the index.

## Required Environment

This skill depends on files in this repository:
- the `sdd-doc-set` convention at `.claude/conventions/sdd-doc-set.md` (what belongs in
  the index and the ID scheme);
- the `INDEX.md` template at `.claude/sdd/templates/docs/INDEX.md`.

If the docs root cannot be located, report it as a blocker.

## Inputs

- The docs root: `docs/` at the WhisperPilot repository root.

## Procedure

Apply the Stop Conditions throughout; halt and report when any is met.

1. Scan the docs root for present main docs and extension docs. In WhisperPilot, register
   extension docs when present
   results and is not registered as a document.
2. Scan `features/` for `F<NNN>_*` folders; for each, count requirements, tasks, and
   scenarios by their IDs. Do not infer or copy TaskPilot status into the index.
3. Scan `decisions/` for `ADR-*` files and read each status.
4. Render `INDEX.md` from the template: the document table (present docs only), the feature
   registry, and the decision log.
5. Preserve human-curated "read when" descriptions and notes where they already exist;
   replace only the generated registry rows.
6. Flag traceability gaps surfaced while counting (a feature with no scenarios, an ADR with
   no status) without resolving them.

## Stop Conditions

Stop and report a blocker when the docs root cannot be located or is not a recognizable
SDD doc tree.

## Output Contract

Emit:

`Skill: sdd-index-sync - output below`

Then include:

| Field | Content |
| --- | --- |
| Status | `completed`, `blocked`, or `skipped` |
| Docs registered | Count and names |
| Features registered | Count and IDs |
| ADRs registered | Count and IDs |
| Gaps flagged | Traceability gaps found, or `none` |
| Blockers | Unresolved issues, or `none` |
