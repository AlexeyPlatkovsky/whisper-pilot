---
name: sdd-feature-author
description: Scaffolds or updates one feature folder (requirements, tasks, scenarios) as a unit in a docs/ SDD tree, with stable feature, requirement, task, and scenario IDs and traceability links. Use when adding or revising a feature.
---

## Scope

- Create or revise exactly one feature folder per run at `features/F<NNN>_<short-name>/`,
  producing or updating `requirements.md`, `tasks.md`, and `scenarios.md` together.
- Assign and preserve IDs per the doc-set convention: feature `F<NNN>`, requirement
  `F<NNN>-R<n>`, task `F<NNN>-T<n>`, scenario `F<NNN>-S<n>`.
- Maintain traceability: each requirement links up to an `idea.md` scope item or
  `roadmap.md` entry, and down to at least one task and one scenario.
- Do not edit main or extension docs, rebuild `INDEX.md`, or renumber existing IDs.

## Required Environment

This skill depends on files in this repository:
- the `sdd-doc-set` convention at `.claude/conventions/sdd-doc-set.md` (feature-folder
  schema, ID scheme, traceability spine, TaskPilot cross-referencing rule);
- the feature templates under `.claude/sdd/templates/features/F000_template/`.

If either is unavailable, report it as a blocker before writing.

Feature folders live under `docs/features/`. TaskPilot (`WP-<n>`) remains the sole tracker
of work status per `AGENTS.md`: when a TaskPilot item exists for a task, reference its
`WP-<n>` ID in the `tasks.md` row; do not duplicate TaskPilot workflow states there.

## Inputs

- Feature intent and short name.
- The `idea.md` scope item or `roadmap.md` entry the feature serves.
- Mode: `new` or `revise` (with the existing feature ID when revising).
- Manager Route run, lifecycle/Git artifacts, confirmed plan row and version or
  another stable approved-source identity, and author-attempt number starting
  at `1`. Increment for every later invocation of the same Route run and plan
  row/source identity; after attempt `1`, require the previous labeled artifact.
- Approved source evidence: a matching versioned discovery draft, `No gaps`
  verifier artifact, and user approval record; a confirmed adoption/bootstrap
  plan row; or an explicit direct user approval record.
- Reload-verified `taskpilot-work` lookup evidence mapping every task with an
  existing TaskPilot item to its ID, type, and parent. Use `TaskPilot: not
  created` only when that lookup explicitly confirms absence.
Never invent IDs or behavioral facts; block on an identity, approval, type, or
parent mismatch.

## Procedure

Apply the Stop Conditions throughout; halt and report when any is met.

1. Complete all input, source, TaskPilot, target-mode, collision, and
   traceability preflight checks before writing. Scan existing feature folders
   and anchors. In `new` mode use `F001` when the tree is empty, otherwise
   `max(existing F number)+1`; normalize the short name to lowercase ASCII
   kebab-case. In `revise` mode resolve by ID and preserve the existing folder
   name unless the user explicitly authorizes a rename.
2. Treat an existing ID as the row's stable identity. Never remove or reuse an
   assigned ID. Keep an obsolete requirement/task table row in place and put
   `Superseded: yes — <replacement ID or reason>` inside its Requirement or
   Task cell. For a scenario, put that exact line immediately below its
   `### F<NNN>-S<n>:` heading. Allocate a new ID
   as the maximum active-or-superseded numeric ID in that namespace plus one,
   starting at `1`, and preflight uniqueness/collisions in every namespace.
3. Prepare `requirements.md`: summary, the served `idea`/`roadmap` link, requirements with
   `F<NNN>-R<n>` IDs, acceptance criteria, constraints, and explicit out-of-scope.
4. Prepare `tasks.md`: tasks with `F<NNN>-T<n>` IDs, each linked to the requirement(s) it
   implements, dependencies, and either the verified TaskPilot ID or
   `TaskPilot: not created`.
5. Prepare `scenarios.md`: Gherkin scenarios with `F<NNN>-S<n>` IDs linked to requirements,
   plus a manual verification checklist.
6. For active rows only, verify each requirement has at least one active task
   and scenario. Flag any active requirement, task, or scenario that lacks a
   required link as a blocking traceability gap.
7. Validate all prepared content before mutation, then apply all three file
   changes in one atomic patch. If the available editor cannot do so, block
   before mutation. Re-read and validate all three files. A post-write failure
   is `recovery required`, not `blocked`; preserve the actual tree and report
   exact recovery work. Note that `INDEX.md` needs re-syncing; do not edit it
   here.

## Stop Conditions

Stop and report a blocker when:
- an active requirement cannot be traced up to an `idea.md` or `roadmap.md` item;
- two active requirements directly conflict;
- the chosen feature ID or slug collides with an existing feature.
- any active requirement, task, or scenario lacks the required traceability link.

Validate superseded rows only for retention, marker placement, ID uniqueness,
and non-reuse; their historical behavior does not participate in active
conflict or traceability gates.

## Output Contract

Emit:

`Skill: sdd-feature-author - output below`

Then include:

| Field | Content |
| --- | --- |
| Route / source / attempt | Manager Route run, plan row/version or approved-source identity, author-attempt number |
| Status | `completed`, `blocked`, or `recovery required` |
| Feature | `F<NNN>_<short-name>` |
| Mode | `new` or `revise` |
| Requirements | Active count and IDs |
| Tasks | Active count and IDs |
| Scenarios | Active count and IDs |
| Superseded IDs | IDs retained with the exact superseded marker, or `none` |
| Traceability gaps | Active requirements without a task/scenario, tasks without a requirement, scenarios without a requirement, or `none` |
| INDEX sync needed | `yes` for completed or recovery required; `no — preflight blocked with no mutation` for blocked |
| Blockers | Unresolved issues, or `none` |

For `recovery required`, also list actual changed files, the failed validation,
and the exact recovery action. Callers must stop and preserve that state until
the recovery succeeds.
