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

If the docs root, convention, or template is missing, unreadable, or malformed,
report it as a blocker.

## Inputs

- The docs root: `docs/` at the WhisperPilot repository root.
- Mode `create` when `docs/INDEX.md` is absent or `update` when it exists.
- The exact manager Route run and sync-attempt number. Start at `1`; increment
  after a retry or documentation rework invalidates a prior sync; every attempt
  after `1` also requires the previous labeled sync artifact.

## Procedure

Apply the Stop Conditions throughout; halt and report when any is met.

1. Scan the docs root for present main docs and recognized extension docs. Register only
   files that exist; do not treat build output, test results, or TaskPilot records as docs.
2. Scan `features/` for `F<NNN>_*` folders; for each, count requirements, tasks, and
   scenarios by their active IDs. Exclude rows carrying the exact
   `Superseded: yes — <replacement ID or reason>` marker in the
   Requirement/Task cell, or immediately below a scenario heading, from active
   counts; validate that their IDs remain present, unique, and unreused. Do not
   infer or copy TaskPilot status into the index.
3. Scan `decisions/` for `ADR-*` files and read each status.
4. Validate the complete scanned source, ID/supersession rules, required feature
   files, folder names, permitted ADR statuses, and prepared source-to-index
   render before mutation.
5. In `create` mode, require the index to be absent and render it from the
   template with its generated markers. In `update` mode, require the index to
   exist and replace only sections delimited by unique generated markers.
   A mode/existence mismatch or missing/duplicated update marker blocks.
6. Re-read the result and prove one-to-one correspondence with the scanned tree
   while preserving curated sections. A post-write failure is `recovery
   required`; report the actual index state and exact recovery action.

## Stop Conditions

Stop and report a blocker when the docs root cannot be located or is not a recognizable
SDD doc tree.

## Output Contract

Emit:

`Skill: sdd-index-sync - output below`

Then include:

| Field | Content |
| --- | --- |
| Route run / attempt | Exact manager Route run and sync-attempt number |
| Mode | Exact `create` or `update` mode used |
| Status | `completed` after verified write, `blocked` before mutation, or `recovery required` after a failed post-write check |
| Docs registered | Count and names |
| Features registered | Count and IDs |
| ADRs registered | Count and IDs |
| Structural/index gaps | ID, required-file, marker, or source-to-index gaps, or `none`; full link traceability is not assessed |
| Blockers | Unresolved issues, or `none` |
| Validation | Source-to-index comparison and curated-content preservation result |

For `recovery required`, include the actual `docs/INDEX.md` state and exact
recovery action. Callers stop downstream work, preserve the tree, increment the
sync attempt with the prior artifact, and rerun after recovery.
