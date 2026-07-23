---
name: verify-readiness
description: Definition-of-Ready gate — verify a routed work item carries every artifact an AI agent needs to implement it correctly in one run, and STOP for an explicit user disposition (ignore / skip / create) on any gap before implementation begins.
---

# Skill: verify-readiness

## Purpose

Confirm that a routed work item is **Ready** for implementation: it carries every artifact an AI agent with no conversation context needs to build it correctly in a single run. A missing or vague readiness artifact is the root cause of post-hoc rework, so this gate **blocks and asks** rather than guessing.

This skill verifies that readiness artifacts **exist and are specific**. It does not author them and it does not re-approve scope — authoring is owned by `discover-feature`; scope approval is owned by `discover-feature`.

## When This Skill Applies

Use when a pipeline routes a Definition-of-Ready check before implementation begins — currently Step 0 of `.claude/pipelines/implement-feature.md`.

Do not use:
- For trivial or exempt work that never entered a pipeline.
- To re-litigate already-approved scope. A spec approved through `discover-feature` that still carries all readiness artifacts passes here without re-discovery.

## Required Input

The manager or pipeline supplies one existing TaskPilot item record and, for a child, its parent context. If either required input is absent or unreadable, report `blocked`.

## Procedure

1. Read the supplied work item and, for a child task, its parent feature/epic.
   Inspect its self-contained description, `dod`, optional `dor`, structural
   links, and item type. Do not depend on a legacy phase or feature task
   specification.
2. Evaluate every DoR criterion below. A criterion passes only if the artifact is **present and specific** — a restated title, a placeholder, or "TBD" does not pass.
3. **If all criteria pass:** emit the output artifact with status `completed` / `Ready` and report. The routing pipeline advances.
4. **If any criterion fails: do not implement.** For each gap, ask the user which disposition applies (see Dispositions), collect the answer, then emit the output artifact with status `blocked` listing each gap and its chosen disposition.
5. The routing pipeline resolves each gap per its disposition and re-runs this gate. A disposition directs remediation and is recorded on the item; it does not make a missing criterion pass. Do not report `Ready` until every criterion passes.

## DoR Criteria

The item is Ready only when all of the following pass:

1. **Detailed description** — a narrative an agent with no conversation context can act on: user goal, happy path, primary failure mode, and scope boundary, all explicit.
2. **Observable behavior at the item's altitude** — a `task` or `feature`
   includes at least one Given/When/Then scenario; an `epic` instead includes a
   concrete child-feature breakdown. A task description states its happy path,
   failure behavior, and edge cases; a feature or epic may defer detail only by
   naming the child that owns it. For a UI task with no deeper logic, accept the
   UI smoke path defined by `.claude/skills/discover-requirements/SKILL.md`.
3. **DoD checklist** — the item's `dod` contains at least two objectively
   pass/fail completion checks.
4. **Parent context** — for a child task, the parent feature/epic exists and is readable; for a standalone task, that is stated explicitly.
5. **Named target surfaces** — target files, modules, commands, or interfaces named when known, or an explicit "to be identified during implementation" so the omission is deliberate, not accidental.
6. **Constraints** — performance, security, platform (macOS/Windows), and — for UI surfaces — the interaction contract are stated, or an explicit "none."

## Dispositions

When a criterion fails, ask the user which disposition applies to that gap. The three differ by their effect on deliverable scope:

- **ignore** — only for a supporting artifact that is not required by any of the six DoR criteria. Record the omission on the item, then re-evaluate the criteria; `ignore` cannot waive a required readiness criterion.
- **skip** — removes a portion of deliverable scope. Update the item's description and DoD to record the narrower scope, then re-run this gate against the narrowed item.
- **create** — report the missing artifact and return control to the manager for the next route. This skill does not invoke sibling skills.

Never proceed past a gap with "I'll assume X." A required readiness gap is resolved only when the item is updated and the affected criterion passes on re-run.

## Output Contract

Begin the artifact with:

`Skill: verify-readiness - output below`

Then report status (`completed` when Ready, `blocked` when any criterion is incomplete) and a per-criterion table:

| Criterion | Result | Gap / Disposition |
|-----------|--------|-------------------|
| 1. Detailed description | pass / blocked | — or `<gap>` → ignore / skip / create |
| 2. Observable behavior at item altitude | pass / blocked | … |
| 3. DoD checklist (`dod`, minimum 2) | pass / blocked | … |
| 4. Parent context | pass / blocked | … |
| 5. Named target surfaces | pass / blocked | … |
| 6. Constraints | pass / blocked | … |

End with the gate verdict line: `DoR gate: Ready` or `DoR gate: Blocked — <n> unresolved criterion/criteria`.

The routing pipeline branches on this `DoR gate:` verdict line: `status: completed` corresponds to `Ready`, and `status: blocked` to `Blocked`.
