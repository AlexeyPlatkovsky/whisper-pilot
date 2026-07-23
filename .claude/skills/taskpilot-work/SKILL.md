---
name: taskpilot-work
description: Manage WhisperPilot TaskPilot work items, including item lookup, approved creation, canonical item fields, lifecycle metadata, links, and validation. Use for TaskPilot-only administration and whenever routed work needs its TaskPilot record prepared or updated.
---

# Skill: taskpilot-work

## Purpose

Keep TaskPilot records complete and canonical without changing product artifacts.

## Lifecycle Model

This skill owns all TaskPilot mutations for tracked work. A conversation
artifact, a passing test, or a Git commit does **not** change an item status.
Only a verified mutation through this skill does.

| Status | Meaning | Permitted next state |
|---|---|---|
| `backlog` | Not actively being specified or delivered | `in_progress` (discovery), `ready` (verified existing scope) |
| `in_progress` | An approved discovery or implementation run is active | `ready`, `blocked`, `done` |
| `ready` | Approved scope and DoR are complete; eligible for implementation | `in_progress`, `blocked` |
| `blocked` | Progress requires an external decision, dependency, or remediation | `ready`, `in_progress` (explicit resume) |
| `done` | DoD and completion evidence passed and the completion mutation was reload-verified | none |

`in_progress → done` is permitted only for an implementation task after every
required completion gate passes. Discovery closes `in_progress → ready`, never
to `done` merely because the discovery conversation finished.

## Procedure

1. Inspect existing items before creating one: `taskpilot --json item list` and
   `taskpilot item show <ID>`. For a child, inspect its parent feature/epic too.
2. If no item covers tracked non-trivial work, obtain user approval before
   creating it. Use an explicit type and create it in `backlog`. AI-governance
   maintenance exempted by `AGENTS.md` must not create an item unless the user
   explicitly requests TaskPilot administration.
3. Keep the canonical YAML record implementation-ready: a self-contained
   description, objective `dod`, and `dor` when a dependency or approval remains
   unresolved. Use `apply_patch` for `dod` or `dor` because the CLI does not own
   those fields.
4. For **every** create, field, relationship, comment, or lifecycle mutation:
   reload the affected records first; validate identity and relationship
   invariants; make one intended mutation; run `taskpilot validate`; reload; and
   verify the expected fields and status. Reject self-links and parent/blocker
   cycles. A failure after partial mutation must report the actual reloaded
   state and `recovery required`; retry only a missing idempotent step whose
   prior effect is known.

### Lifecycle Operations

#### Start discovery

For an approved discovery run, transition the selected `backlog` item to
`in_progress`. Add a start comment naming the run and parent context. When the
approved spec has been persisted and DoR passes, transition it to `ready`.

#### Promote an existing ready-to-implement item

For a pre-existing `backlog` item whose scope was already approved, do not force
a redundant discovery run. Verify DoR against the item and parent context, add a
comment identifying that evidence, and perform the verified `backlog → ready`
transition. If DoR is incomplete or the scope is no longer approved, route
discovery instead.

#### Start implementation

Before implementation edits, confirm the selected item is `ready` (or is a
user-approved resumed `blocked` item), then transition it to `in_progress`.
Add a start comment identifying the active branch and, when applicable, the
declared delivery cohort. Do not implement if this artifact is absent.

#### Block and resume

When an external decision, dependency, or a non-remediable gate blocks the run,
transition the active item to `blocked` and comment with the cause, evidence,
and the exact unblocking action. Resuming requires an explicit disposition and
a verified transition to `ready` (waiting work) or `in_progress` (active work).

#### Complete implementation

After `task-quality` passes the prepared completion-evidence record, add that
record as the completion comment, transition the item to `done`, validate,
reload, and prove `done`. Stop there. Staging, committing, and commit-failure
recovery belong exclusively to `work-with-git`. `task-complete` must not run
before both lifecycle verification and the separate atomic-commit artifact
succeed.

## Batch And Hierarchy Rules

A branch is a workspace, not a TaskPilot identity. For a user-approved
feature/epic batch, prepare a visible batch manifest before the first child
starts: root item, ordered child IDs, their statuses, branch, and the one active
child. A normal batch has one executable leaf task at a time; the active epic,
feature, and leaf task form its single `in_progress` path.

The only exception is a predeclared **delivery cohort** of at most two sibling
tasks that must ship in one local commit. Both must be `ready`, technically
inseparable, named in the batch manifest before edits, validated against their
own DoD, and linked to the same commit hash. If either task blocks or fails DoD,
split the cohort into separate commits or leave both items unfinished; never
mark only one cohort member done from the shared delivery.

Starting a child moves an otherwise non-active parent feature/epic to
`in_progress` with a roll-up comment. A blocked child does **not** block its
parent; the parent remains `in_progress` and records the child blocker. A parent
may transition to `done` only after all required direct children are `done`, its
own DoD passes, and a roll-up completion comment links the child evidence.

### Persist an Approved Discovery Spec

Given a prepared canonical-record update, freshly reload the item and compare
its `id`, `type`, `status`, and parent with the update's expected invariants.
Return `blocked` without mutation on any mismatch. Then snapshot all protected
metadata: `id`, `type`, `status`, parent, relations, blockers, tags, phase,
comments, and unknown extension fields. Update only the approved title,
description, and `dod`, validate the
workspace, reload the item, and report whether the persisted fields and protected
metadata respectively match the approved update and the pre-mutation snapshot.
This skill owns the TaskPilot mutation and
validation; the discovery-record skill only prepares the update. Persisting the
spec does not itself make the item `done`; the discovery lifecycle operation
sets it to `ready` only after DoR passes.

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
| Inspect | `taskpilot item show WP-1` |
| Create | `taskpilot item create --title "Title" --type task --status backlog` |
| Start / resume | `taskpilot item update WP-1 --status in_progress` |
| Mark ready | `taskpilot item update WP-1 --status ready` |
| Block | `taskpilot item update WP-1 --status blocked` |
| Complete | `taskpilot item update WP-1 --status done` |
| Parent | `taskpilot item parent CHILD-ID PARENT-ID` |
| Block | `taskpilot item blocks BLOCKER-ID BLOCKED-ID` |
| Relate | `taskpilot item relates SOURCE-ID TARGET-ID` |
| Comment | `taskpilot item comment WP-1 "Validation and smoke results"` |
| Validate | `taskpilot validate` |

## Output Contract

`Skill: taskpilot-work - output below`

`Route run: <manager route/run identifier>`

| Operation | Operation result | Item(s) | Before → after | Evidence / verification |
|---|---|---|---|---|
| create / inspect / persist_spec / start_discovery / mark_ready / start_implementation / block / resume / complete / link / comment / phase_lookup | completed / blocked / recovery required | `<ID(s)>` / none | `<exact fields/status>` / N/A | preflight, mutation, `taskpilot validate`, reload comparison, and blocker/recovery detail |

`Operation result` describes this skill execution only. It must never be used
as a substitute for the item’s TaskPilot lifecycle status.
