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

If either artifact is absent, report `Blocked` and stop before Stage 1.

## Inputs

- Source of project intent (user description, brief, or notes).
- Chosen tier: `Lean`, `Standard` (default), or `Full`.
- The docs root: `docs/` at the WhisperPilot repository root.

## Stages

| Stage | Capability | Required Visible Artifact |
| --- | --- | --- |
| 0. Activate TaskPilot item | `Skill: taskpilot-work` — verified `ready → in_progress` before artifact edits | `Skill: taskpilot-work - output below` with reloaded `in_progress` evidence |
| 1. Intake | direct — confirm tier, docs root, and source of intent | none |
| 2. Idea | `Skill: sdd-doc-author` (idea.md) | `Skill: sdd-doc-author - output below` |
| 3. Architecture | `Skill: sdd-doc-author` (architecture.md, + extension docs if warranted) | `Skill: sdd-doc-author - output below` |
| 4. Design (Standard+) | `Skill: sdd-doc-author` (design.md) | `Skill: sdd-doc-author - output below` |
| 5. Testing (Standard+) | `Skill: sdd-doc-author` (testing.md) | `Skill: sdd-doc-author - output below` |
| 6. Roadmap | `Skill: sdd-doc-author` (roadmap.md) | `Skill: sdd-doc-author - output below` |
| 7. Features (Standard+) | `Skill: sdd-feature-author` (once per feature) | `Skill: sdd-feature-author - output below` |
| 8. Index | `Skill: sdd-index-sync` | `Skill: sdd-index-sync - output below` |
| 9. Review | `Agent: sdd-spec-reviewer` | `Agent: sdd-spec-reviewer - output below` |
| 10. Suggest companions | direct — present the bundle's `RECOMMENDS.md` companions if that file exists under `.claude/sdd/`; otherwise record that none ship with this adoption | a note of companions offered and which were adopted, or `none offered` |
| 11. Definition of Done | `Skill: task-quality` | `Skill: task-quality - output below` with `Quality gate: pass` |
| 12. TaskPilot completion and local commit | `Skill: taskpilot-work`, then `Skill: work-with-git` — verified `in_progress → done`, then one atomic local commit | `Skill: taskpilot-work - output below` with reloaded `done` evidence and commit hash |
| 13. Task Complete | `Skill: task-complete` | `Skill: task-complete - output below` |

On the `Lean` tier, skip stages 4, 5, and 7. Stage 10 is opt-in and may be declined. Do not
advance past a stage whose expected visible artifact is missing or whose
Definition-of-Done gate is not `pass`.

## Authority Sources

- the `sdd-doc-set` convention at `.claude/conventions/sdd-doc-set.md`
- the templates under `.claude/sdd/templates/`

## Stop Conditions

- Tier or docs root is ambiguous — return to stage 1.
- A doc-authoring step blocks (ownership conflict, unverifiable facts) — resolve before
  advancing.
- `sdd-spec-reviewer` verdict is `Needs revision` — fix the cited findings, re-run the
  affected authoring stage and stage 8, then re-run stage 9.
- The convention cannot be read — stop and report the missing source.

## Output Contract

Each stage emits the visible artifact listed above; the final closure artifact is required.
