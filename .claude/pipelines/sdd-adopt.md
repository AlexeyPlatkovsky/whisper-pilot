---
name: sdd-adopt
description: Sequences SDD gap analysis, authoring, and review to introduce or expand an SDD docs/ tree in a project that already has documentation or code.
---

# Pipeline: SDD Adopt

## Purpose

Pre-defined routing plan for introducing or expanding a Spec-Driven Development `docs/` tree
in a project that already has documentation, code, or both. It sequences the SDD bundle's
gap analysis, authoring skills, and review so existing material is reconciled rather than
discarded.

This pipeline is a routing artifact. It sequences existing capabilities. It does not
implement step logic and does not emit its own output artifact.

## When to Apply

- The project already has substantive documentation or code and the user wants to adopt or
  expand the SDD doc set.
- Use the bootstrap pipeline instead when starting from no docs.

## Preconditions

- `Manager: manager - output below` with an existing TaskPilot ID in `ready`
  status (or an explicitly approved resumed `blocked` item).
- `Skill: work-with-git - output below` reporting the completed branch decision.

If either artifact is absent, report `Blocked` and stop before Stage 0; perform
no TaskPilot or file mutation.

## Inputs

- The WhisperPilot repository root.
- Complete authoritative documentation tree identified by `AGENTS.md`, plus
  any other locations the user names; record inspected, missing, and excluded
  sources.
- Fixed WhisperPilot tier `Standard` and any user scope constraint.

## Stages

Maintain a `Route execution record` bound to the manager Route run. For every
planned stage/row, record its stable stage ID, visible artifact label or
declared skip, attempt (starting at `1`, incremented after invalidating rework),
SHA-256 of exact artifact text, and terminal status.
For a declared conditional skip, record the exact evaluated condition and
status `skipped`, with artifact label, attempt, and digest `N/A`.

| Stage | Capability | Required Visible Artifact |
| --- | --- | --- |
| 0. Activate TaskPilot item | verified `ready → in_progress` or approved `blocked → in_progress` | operation-specific `taskpilot-work` artifact |
| 1. Intake | confirm repo root, complete source inventory, Standard tier, scope | `SDD adoption intake record` |
| 2. Gap analysis | `sdd-gap-analyzer` | `Pass` or `Needs user decision` |
| 3. Confirm plan | resolve decision rows and version the plan | `Confirmed adoption plan` with plan version and stable row IDs, target, mode, source, dependencies |
| 4. Reconcile docs | one `sdd-doc-author` invocation per confirmed `create new` or `migrate content` doc row; retain `reuse as-is` and optional `not needed` rows as no-action evidence | one matching `Status: completed` artifact per authoring row plus the reviewed no-action rows |
| 5. Features | one `sdd-feature-author` invocation per confirmed feature row | one matching `Status: completed` artifact per row |
| 6. Index | `sdd-index-sync`; pass mode, Route run, and attempt `1` initially, then prior artifact plus incremented attempt after rework | artifact with matching mode and `Status: completed` |
| 7. Review | `sdd-spec-reviewer` | `Pass` or dispositioned `Pass with minor findings` |
| 8. Definition of Done | prepare evidence, then `task-quality` | `Quality gate: pass` |
| 9. TaskPilot completion and local commit | `taskpilot-work`, then `work-with-git` | separate reloaded-`done` artifact and `Local commit evidence - output below` |
| 10. Task Complete | `task-complete` | `Skill: task-complete - output below` |

At stage 8, pass the manager objective DoD, reloaded `in_progress` item, Route
execution record, prepared criterion mapping, and latest accepted authoring,
index, review, and validation artifacts to `task-quality`.

Author actionable docs in confirmed-plan dependency order. Do not invoke
`sdd-doc-author` for `reuse as-is` or `not needed` rows. Do not advance past a stage
whose expected artifact and accepted status are missing. Stop when the
gap-analysis verdict is `Blocked` or the Definition-of-Done
gate is not `pass`.
Pass the confirmed plan version, matching stable row ID, manager Route run, and
author-attempt number to every doc-author and feature-author invocation.

## Authority Sources

- the `sdd-doc-set` convention at `.claude/conventions/sdd-doc-set.md`
- the templates under `.claude/sdd/templates/`
- the stage 2 gap-analysis output

## Stop Conditions

- `Needs user decision` advances only to stage 3; `Blocked` stops.
- A doc-authoring or feature step blocks — resolve before advancing.
- A feature-author result is `recovery required` — stop all downstream stages,
  preserve the reported tree, perform only its exact recovery action, increment
  the author attempt with the prior artifact, and re-run stage 5 before index
  sync.
- An index-sync result is `recovery required` — stop downstream stages,
  preserve the reported tree, perform only its exact recovery action, increment
  the sync attempt with the prior artifact, and re-run stage 6.
- `sdd-spec-reviewer` verdict is `Needs revision` — fix the cited findings, re-run the
  affected stage and stage 6, then re-run stage 7.
- Any other `Blocked`, `skipped`, malformed, or unrecognized outcome stops.
  After activation, persist the blocker and exact unblocking action through
  `taskpilot-work`.
- The convention or repository root cannot be read — stop and report the missing source.

## Output Contract

Each stage emits the visible artifact listed above; the final closure artifact is required.
