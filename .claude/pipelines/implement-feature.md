---
name: implement-feature
description: Ordered execution path for implementing a Tauri + React + TypeScript feature in WhisperPilot.
---

# Pipeline: implement-feature

## Purpose

Sequence the steps for implementing a non-trivial Tauri/React/Rust feature: readiness verification, design resolution, implementation, local validation, documentation maintenance, and closure.

## Preconditions

Before this pipeline begins:
- The manager has classified the task as non-trivial and selected this pipeline.
- `Manager: manager - output below` artifact is present in the conversation.
- An existing TaskPilot ID is present in the manager artifact.
- `Skill: work-with-git - output below` reports the completed branch decision.

## Execution Contract

Execute all steps in this pipeline sequentially without pausing for user input,
unless (a) the applicable step requires a stop or return for its gate verdict,
(b) a step instructs return to a prior step, or (c) a step explicitly requires
user confirmation before advancing (Step 0 DoR gap dispositions, Step 1
brainstorm decision summary).

A git commit, a validate PASS, or any intermediate output artifact does not mark this pipeline complete. Only `Skill: task-complete - output below` from the task-complete step (Step 10) closes the pipeline.

---

## Steps

### Step 0 — Definition of Ready (DoR) Gate

**Runs first, always — before any other step.** Confirm the routed item carries every readiness artifact an AI agent with empty context needs to implement it correctly in a single run.

Skill: `.claude/skills/verify-readiness/SKILL.md`

Required input: existing TaskPilot ID from `Manager: manager - output below`
Required output: `Skill: verify-readiness - output below`

If the verdict is `Ready`, advance to Step 1. If it is `Blocked`, do not implement: resolve each gap per the disposition the skill recorded — for a **create** disposition on a requirements/scope gap, return to the manager to re-route `discover-feature`; for **ignore**, record the non-required omission and re-evaluate; for **skip**, update the item's narrowed scope and DoD — then re-run this gate. Do not advance to Step 1 until the verdict is `Ready`.

---

### Step 1 — Brainstorm (conditional)

**Trigger:** open design decisions exist for this feature.
**Skip:** design is already fully resolved.

Skill: `.claude/skills/brainstorm/SKILL.md`
Required output: `Skill: brainstorm - output below` (decision summary confirmed by user)

Do not advance to Step 2 until the confirmed decision summary is present.

---

### Step 2 — Test Authoring And Red Evidence

Skill: `.claude/skills/testing-pro/SKILL.md`
Required output: `Skill: testing-pro - output below`

For non-trivial logic, apply `.claude/skills/testing-pro/SKILL.md` through the skill.
Do not advance until its required evidence is complete.

---

### Step 3 — Implement

Skill: `.claude/skills/implement-tauri-feature/SKILL.md`
Required output: `Skill: implement-tauri-feature - output below`

Consult `react-tauri-expert` reference topics during implementation.
For UI/interaction surfaces, the implementation skill must produce an interaction contract (drag/click-vs-drag, keyboard, sizing, empty/loading/error states) stating the default/initial state and a user-visible outcome.
For a **novel UI/interaction pattern** (no existing precedent), check prior art before inventing one.
Before advancing, run the four change-hygiene audits in `.claude/conventions/react-tauri/change-hygiene.md` — advisory here; enforced in Step 7.

**Post-implementation validation** — before advancing to Step 4, run local CI-equivalent checks on the current tree:

Skill: `.claude/skills/validate/SKILL.md`
Required output: `Skill: validate - output below`

Select checks matching the touched layers. For a typical feature touching both front-end and Rust:
`checks="lint format tsc vitest coverage clippy nextest cargobuild"`

If validation fails, fix and re-run. Do not advance to Step 4 until `Skill: validate - output below` reports all checks PASS.

---

### Step 4 — UI Verification (conditional)

**Trigger:** implementation touched a visual UI or interaction surface.
**Skip:** no visual UI or interaction surface changed.

Manually verify the changed UI: run the app, exercise the affected states, and
confirm they match the specification and the conventions in
`.claude/conventions/react-tauri/`. Record the states checked and the outcome.

If a state does not match, return to Step 3. If it cannot be assessed from
available inputs, stop and report the blocker; if the limitation is external,
record it and continue to Step 5.

---

### Step 5 — Dedicated Validation

Agent: `.claude/agents/test-runner.md`

Required output: `Agent: test-runner - output below`

The agent runs locally whichever build/test/manual checks apply to the touched layers. If validation fails, return to Step 3. If validation is `Blocked`, continue only when its artifact explicitly identifies an external verification limitation; otherwise stop and report the blocker. Record a permitted limitation and continue to Step 6.

For UI/interaction changes, the agent consumes the Step 4 manual UI verification
record. It requires a passing result unless that record explicitly identifies an
external verification limitation, which it records without treating as a fail.

---

### Step 6 — Design Self-Review (conditional)

**Trigger:** implementation touched a visual UI or interaction surface.
**Skip:** no visual UI or interaction surface changed.

Self-review the visual result against the `.claude/conventions/react-tauri/`
accessibility, performance, and platform-scope conventions. If it needs revision,
return to Step 3. If it cannot be assessed, stop and report the blocker; record a
permitted external limitation and continue to Step 7.

---

### Step 7 — Code Review

Agent: `.claude/agents/code-reviewer.md`
Required output: `Agent: code-reviewer - output below`

If verdict is `Needs revision` or `Blocked` because TDD provenance is missing or invalid, return to Step 2. If verdict is `Needs revision` for another finding, return to Step 3. For any other `Blocked` verdict, stop and report the blocker.
Do not advance to Step 8 until verdict is `Approved` or `Approved with minor notes`.

---

### Step 8 — Documentation Maintenance

Skill: `.claude/skills/documentation-maintenance/SKILL.md`
Required output: `Skill: documentation-maintenance - output below`

Do not advance to Step 9 until this artifact is present.

---

### Step 9 — Definition of Done (DoD) Gate

Run the DoD quality gate to verify all acceptance criteria pass, smoke checklist is complete, and edge cases are covered.

Skill: `.claude/skills/task-quality/SKILL.md`

Required input: existing TaskPilot ID from `Manager: manager - output below`
Required output: `Skill: task-quality - output below`

If the verdict is `blocked`, fix gaps and re-run. Do not advance to Step 10 until the quality gate reports `pass`.

---

### Step 10 — Task Complete

Skill: `.claude/skills/task-complete/SKILL.md`
Required output: `Skill: task-complete - output below`
