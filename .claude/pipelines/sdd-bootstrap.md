---
name: sdd-bootstrap
description: Sequences SDD authoring and review to establish an SDD docs/ tree in a project from scratch.
---

# Pipeline: SDD Bootstrap

## Purpose

Pre-defined routing plan for establishing a Spec-Driven Development `docs/` tree in a
project from scratch. It sequences the SDD bundle's skills and agents so the result conforms
to the `sdd-doc-set` convention.

This pipeline is a routing artifact. It sequences existing capabilities. It does not
implement step logic and does not emit its own output artifact.

## When to Apply

- The project has no SDD docs, or only an empty/placeholder docs tree, and the user wants to
  create the specification set from scratch.
- Use the adoption pipeline instead when the project already has substantive documentation
  or code to reconcile. WhisperPilot already has `docs/idea.md`, `docs/architecture.md`, and
 a working codebase, so `sdd-adopt` is the normal entry point
  here; apply this pipeline only to a genuinely empty docs root.

## Preconditions

- `Manager: manager - output below` with an existing TaskPilot ID in `ready`
  status (or an explicitly approved resumed `blocked` item).
- `Skill: work-with-git - output below` reporting the completed branch decision.

If either artifact is absent, report `Blocked` and stop before Stage 0; perform
no TaskPilot or file mutation.

## Inputs

- Source of project intent (user description, brief, or notes).
- Fixed WhisperPilot tier `Standard`; another tier requires a separately routed
  governance change.
- The docs root: `docs/` at the WhisperPilot repository root.

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
| 1. Intake | validate empty docs root, Standard tier, and versioned source inventory | `SDD bootstrap intake record` with plan version and stable row IDs |
| 2. Idea | `sdd-doc-author`, `idea.md`, mode `new` | matching completed artifact |
| 3. Architecture | one invocation per convention-required document | one matching completed artifact per target |
| 4. Design | `sdd-doc-author`, `design.md`, mode `new` | matching completed artifact |
| 5. Testing | `sdd-doc-author`, `testing.md`, mode `new` | matching completed artifact |
| 6. Roadmap | `sdd-doc-author`, `roadmap.md`, mode `new` | matching completed artifact |
| 7. Index | `sdd-index-sync`: `create` only while INDEX is absent; `update` on every post-creation rerun; pass Route run and attempt `1`, then prior artifact plus incremented attempt after rework | matching mode and `Status: completed` |
| 8. Review | `sdd-spec-reviewer` | `Pass` or dispositioned `Pass with minor findings` |
| 9. Definition of Done | prepare evidence, then `task-quality` | `Quality gate: pass` |
| 10. TaskPilot completion and local commit | `taskpilot-work`, then `work-with-git` | separate reloaded-`done` artifact and `Local commit evidence - output below` |
| 11. Task Complete | `task-complete` | `Skill: task-complete - output below` |

At stage 9, pass the manager objective DoD, reloaded `in_progress` item, Route
execution record, prepared criterion mapping, and latest accepted authoring,
index, review, and validation artifacts to `task-quality`.

Do not advance past a stage whose expected visible artifact and accepted status
is missing or whose
Definition-of-Done gate is not `pass`.
Pass the intake plan version, matching stable row ID, manager Route run, and
author-attempt number to every doc-author invocation.
If index sync reports `recovery required`, stop downstream stages, preserve the
reported tree, perform only its exact recovery action, increment the sync
attempt with the prior artifact, and re-run stage 7.

## Authority Sources

- the `sdd-doc-set` convention at `.claude/conventions/sdd-doc-set.md`
- the templates under `.claude/sdd/templates/`

## Stop Conditions

- Docs root or source inventory is ambiguous — return to stage 1.
- A doc-authoring step blocks (ownership conflict, unverifiable facts) — resolve before
  advancing.
- `sdd-spec-reviewer` verdict is `Needs revision` — fix the cited findings, re-run the
  affected authoring stage and stage 7, then re-run stage 8.
- `Blocked`, `skipped`, malformed, or unrecognized outcomes stop. After
  activation, persist the blocker and exact unblocking action through
  `taskpilot-work`.
- The convention cannot be read — stop and report the missing source.

## Output Contract

Each stage emits the visible artifact listed above; the final closure artifact is required.
