---
name: taskpilot-work
description: Manage WhisperPilot TaskPilot work items, including item lookup, approved creation, canonical item fields, lifecycle metadata, links, and validation. Use for TaskPilot-only administration and whenever routed work needs its TaskPilot record prepared or updated.
---

# Skill: taskpilot-work

## Purpose

Keep TaskPilot records complete and canonical without changing product artifacts.

## Procedure

1. Inspect existing items before creating one: `taskpilot --json item list` and
   `taskpilot item show <ID>`.
2. If no item covers non-trivial work, obtain user approval before creating it.
   Use an explicit type and create it in `backlog`.
3. Keep the canonical YAML record implementation-ready: a self-contained
   description, objective `dod`, and `dor` when a dependency or approval remains
   unresolved. Use `apply_patch` for `dod` or `dor` because the CLI does not own
   those fields.
4. Record hierarchy, blockers, and related context structurally. Before closing,
   add validation and smoke evidence as a completion comment.
5. Run `taskpilot validate` after editing a canonical record.

### Persist an Approved Discovery Spec

Given a prepared canonical-record update, snapshot the item identity and protected
metadata, update only the approved title, description, and `dod`, validate the
workspace, reload the item, and report whether the persisted fields and protected
metadata match the approved update. This skill owns the TaskPilot mutation and
validation; the discovery-record skill only prepares the update.

### Phase-Tag Lookup

For a named phase request, run `taskpilot --json item list` and filter the
result by the requested phase tag. Report no registered items to the user; report
an all-done result without routing; otherwise return only unfinished child IDs to
`task-routing` for independent routing. Never create a phase wrapper item.

## Command Reference

| Action | Command |
|---|---|
| List | `taskpilot item list` |
| List JSON | `taskpilot --json item list` |
| Inspect | `taskpilot item show VP-1` |
| Create | `taskpilot item create --title "Title" --type task --status backlog` |
| Update | `taskpilot item update VP-1 --status in_progress` |
| Parent | `taskpilot item parent CHILD-ID PARENT-ID` |
| Block | `taskpilot item blocks BLOCKER-ID BLOCKED-ID` |
| Relate | `taskpilot item relates SOURCE-ID TARGET-ID` |
| Comment | `taskpilot item comment VP-1 "Validation and smoke results"` |
| Validate | `taskpilot validate` |

## Output Contract

`Skill: taskpilot-work - output below`

| Status | Item | Action | Validation |
|---|---|---|---|
| completed / blocked | `<ID>` / none | concise result | `taskpilot validate` result / N/A |
