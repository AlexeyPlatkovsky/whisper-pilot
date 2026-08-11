---
name: task-quality
description: Read-only Definition-of-Done gate that maps each approved behavior, DoD item, scenario, and smoke check to completion evidence before TaskPilot completion.
---

# Skill: task-quality

## Purpose

Verify that every approved completion criterion has explicit evidence before an
item transitions into **done**. This skill validates a prepared
completion-evidence record; `.claude/skills/taskpilot-work/SKILL.md` owns the
later comment and lifecycle mutation.

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
- Untracked user-authored worktree review. Its manager-declared direct route
  must instead state objective DoD criteria for the frozen boundary and record
  validation and code-review evidence in `task-complete`, plus commit evidence
  only when the manager-declared route includes a user-authorized commit; do
  not invent a TaskPilot item to invoke this skill.
- Validating an item at creation time. An item is born in `backlog`; its description and DoD must be completed before its discovery run can transition it to `ready`. Running a DoR check at creation would falsely block newly scoped work.

## Required Input

The invoking pipeline must supply the manager route, active TaskPilot ID, the
reloaded `in_progress` item (including description, scenarios, and `dod`), the
route's latest final-validation/review/manual-verification/documentation
artifacts, and a prepared completion-evidence record mapping every DoD item and
scenario to those artifacts, plus the Route execution record. Verify every
evidence reference against the highest valid attempt/key/digest in that record.
The prepared record is not yet a TaskPilot
comment. If an applicable input is absent, stale, malformed, or belongs to
another item or route run, report `blocked`.

## Procedure

For Definition of Ready, use `.claude/skills/verify-readiness/SKILL.md`; this skill validates only Definition of Done.

### Definition of Done (post-implementation)

Run when an `in_progress` item targets `done`. Evaluate the supplied item and
prepared completion-evidence record.

1. **All described behavior maps to evidence** — every BDD scenario and
   observable behavior has one or more exact test, validation, review, or manual
   record references.
2. **BDD contract present for behavior-bearing runtime scope** — every
   runtime `feature` or `task` item contains at least
   one Given/When/Then scenario. Epic items carry their child-feature breakdown
   instead; a UI-only task may use the UI smoke-path exception defined by
   `.claude/skills/discover-requirements/SKILL.md`.
3. **Smoke checklist complete** — every applicable item below has an expected
   observable result and evidence; every `N/A` row has a scope-based reason.
4. **Runtime boundaries covered** — every negative, positive-boundary, concurrency, and
   lifecycle scenario present in the item maps to evidence.
5. **Final routed validation accepted** — consume the route's final validation
   artifact, normally `Agent: test-runner - output below`; use
   `Skill: validate - output below` only when the manager explicitly declares it
   final.

Pass: every criterion passes and the prepared completion-evidence record is
complete. The invoking pipeline then passes that record to `taskpilot-work`,
which creates the completion comment and performs the verified
`in_progress → done` transition. This gate never requires or creates the final
comment itself.
Block: report each gap.

An external verification limitation does not block DoD when it does not identify
an implementation defect, every independent check passed, and the completion
evidence record names the unavailable check and its cause. After this gate
passes, `taskpilot-work` persists that same accepted record as the completion
comment.

### Canonical Smoke Checklist

| # | Check | Applicability |
|---|---|---|
| 1 | App reaches the expected ready state without crash | A changed runtime path starts or initializes the app |
| 2 | Approved happy-path scenarios reach their stated outcomes | Runtime behavior changed |
| 3 | Approved failure scenarios expose their stated error result without data loss or crash | Error behavior changed or is named in scope |
| 4 | Named empty, null, corrupt, and boundary inputs produce their specified result | Input/data processing changed |
| 5 | Named rapid, repeated, concurrent, or cancellation scenarios preserve the specified state invariant | Stateful or async behavior changed |
| 6 | Named start, stop, restart, and cleanup scenarios reach their specified state | Session, long-running process, or service lifecycle changed |
| 7 | The route's final regression suite passes for every touched runtime layer | Runtime behavior changed |

For documentation, instruction, and TaskPilot-only work, the manager-declared
objective DoD replaces this runtime checklist. Emit one evidence row per
objective criterion and mark runtime BDD/boundary checks `N/A — manager
objective DoD route`.

## Output Contract

`Skill: task-quality - output below`

`Item / route run: <ID> / <run identifier>`

| Criterion / scenario / smoke check | Applicability | Result | Evidence |
|---|---|---|---|
| `<stable ID or exact text>` | applicable / N/A — `<reason>` | pass / blocked | `<execution-record key, latest attempt, digest, artifact label, and test/check/manual-record identity>` |

When an external verification limitation exists, also emit:

| Scope | Cause | Unavailable Coverage | Available Evidence | Implementation Defect Found |
|---|---|---|---|---|

End with `Quality gate: pass` or
`Quality gate: blocked — <n> unresolved criterion/criteria`.
