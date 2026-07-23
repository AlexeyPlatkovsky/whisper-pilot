---
name: task-quality
description: Quality gate that validates TaskPilot item completeness — DoD, BDD scenarios, and smoke checklist — before an item closes as done.
---

# Skill: task-quality

## Purpose

Ensure every WhisperPilot TaskPilot item carries the artifacts needed for correct implementation and verification. This gate runs before an item transitions into **done**. It validates readiness or completion evidence; `.claude/skills/taskpilot-work/SKILL.md` owns every lifecycle mutation.

This skill inspects existing task content. It does not author artifacts — it reports gaps and blocks the transition until resolved.

## When This Skill Applies

Use when:
- A TaskPilot item is proposed for `done` status — validate DoD completeness

Do not use for:
- Trivial work
- TaskPilot-exempt AI-governance maintenance. Its manager-declared direct route
  must instead state an objective instruction-system DoD and record its
  structural validation in `task-complete`; do not invent a TaskPilot item just
  to invoke this skill.
- Validating an item at creation time. An item is born in `backlog`; its description and DoD must be completed before its discovery run can transition it to `ready`. Running a DoR check at creation would falsely block newly scoped work.

## Required Input

The invoking pipeline supplies one existing TaskPilot item record and completion evidence. If either is absent or unreadable, report `blocked`.

## Procedure

For Definition of Ready, use `.claude/skills/verify-readiness/SKILL.md`; this skill validates only Definition of Done.

### Definition of Done (post-implementation)

Run when an item targets `done` status. Verify all its `dod` items plus:

Evaluate the supplied item's description, DoD, evidence, and completion comment.

1. **All described behavior passes** — every BDD scenario and observable behavior in the TaskPilot description has been executed and verified (test or manual), except for an external verification limitation documented with its scope, cause, and available automated evidence.
2. **BDD contract present** — every `feature` or `task` item contains at least
   one Given/When/Then scenario. Epic items carry their child-feature breakdown
   instead; a UI-only task may use the UI smoke-path exception defined by
   `.claude/skills/discover-requirements/SKILL.md`.
3. **Smoke checklist complete** — every applicable item from the canonical
   checklist below is verified; record each inapplicable scoped check with its
   rationale.
4. **Edge cases covered** — negative scenarios from the description have been tested
5. **Local validation passes** — the touched layers build and tests pass (see
   `.claude/skills/validate/SKILL.md`)

Pass: all criteria pass and the item has a TaskPilot completion comment referencing test and verification results. The invoking pipeline must then use `taskpilot-work` to make and reload-verify the `in_progress → done` transition; this gate alone never closes an item.
Block: report each gap.

An external verification limitation does not block DoD when it does not identify
an implementation defect, every independent check passed, and the completion
comment names the unavailable check and its cause.

### Canonical Smoke Checklist

| # | Check | Applicability |
|---|---|---|
| 1 | App starts without crash | Runtime-affecting work |
| 2 | Happy path completes end-to-end | Runtime-affecting work |
| 3 | Error states fail gracefully | Runtime-affecting work |
| 4 | Empty, null, or corrupt input does not crash | Data-processing work |
| 5 | Boundary and rapid/concurrent operations are safe | Stateful or async work |
| 6 | Start, stop, and restart lifecycle works | Session, capture, or service work |
| 7 | Existing behavior has not regressed | Runtime-affecting work |

For documentation, instruction, and TaskPilot-only work, mark every runtime
check N/A with the reason and verify the applicable instruction or metadata
behavior instead.

## Output Contract

`Skill: task-quality - output below`

| Gate | Status | Gaps |
|------|--------|------|
| DoD (post-implementation) | pass / blocked | — or list of gaps |

Then:

`Quality gate: pass` or `Quality gate: blocked — <n> gap(s)`
