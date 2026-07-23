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
- An existing TaskPilot ID in `ready` status (or an explicitly approved resumed
  `blocked` item) is present in the manager artifact.
- `Skill: work-with-git - output below` reports the completed branch decision.
- If the manager assigned **Lite** tier, its artifact also contains the reduced
  readiness confirmation required by `AGENTS.md` and
  `.claude/skills/task-routing/SKILL.md`: `(target identified) and DoD present`.

If any precondition is absent, stop and report the missing artifact or
confirmation. Do not begin Step 0.

## Execution Contract

Execute all steps in this pipeline sequentially without pausing for user input,
unless (a) the applicable step requires a stop or return for its gate verdict,
(b) a step instructs return to a prior step, or (c) a step explicitly requires
user confirmation before advancing (Step 0 DoR gap dispositions, Step 1
brainstorm decision summary).

A git commit, a validate PASS, or any intermediate output artifact does not mark this pipeline complete. Only `Skill: task-complete - output below` from the task-complete step (Step 11) closes the pipeline.

---

## Steps

### Step 0 — Definition of Ready (DoR) Gate

**Runs first, always — before any other step.** Confirm the routed item carries every readiness artifact an AI agent with empty context needs to implement it correctly in a single run.

Skill: `.claude/skills/verify-readiness/SKILL.md`

Required input: existing TaskPilot ID from `Manager: manager - output below`
Required output: `Skill: verify-readiness - output below`

If the verdict is `Ready`, advance to Step 0a. If it is `Blocked`, do not implement: resolve each gap per the disposition the skill recorded — for a **create** disposition on a requirements/scope gap, return to the manager to re-route `discover-feature`; for **ignore**, record the non-required omission and re-evaluate; for **skip**, update the item's narrowed scope and DoD — then re-run this gate. Do not advance to Step 0a until the verdict is `Ready`.

If the run cannot immediately resolve a DoR blocker, invoke
`taskpilot-work` to record the cause and transition the item to `blocked`.

---

### Step 0a — Activate TaskPilot Item

Skill: `.claude/skills/taskpilot-work/SKILL.md`

After DoR is `Ready` and before test or production edits, perform the verified
`ready → in_progress` start operation. The required artifact must name the
active branch and show the reloaded `in_progress` item. In a batch, it must also
name the visible batch manifest and confirm that this is the only active child
unless a two-task delivery cohort was declared before edits.

Do not advance to Step 1 without `Skill: taskpilot-work - output below` showing
the verified lifecycle transition.

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

Before invoking the skill, emit a visible `Touched-layer validation plan` from
the changed-file list in `Skill: implement-tauri-feature - output below`. Select
the ordered, de-duplicated union of these checks; do not select checks by
judgment or omit a mapped check:

| Changed file type | Required checks |
| --- | --- |
| Any `.ts` or `.tsx` file | `lint format tsc vitest build` |
| Any front-end test file matching `*.test.ts`, `*.test.tsx`, `*.spec.ts`, or `*.spec.tsx` | Add `coverage` |
| Any `.rs` file | `clippy rusttest cargobuild` |
| `Cargo.lock`, or a dependency-table entry in `Cargo.toml` | `audit clippy rusttest cargobuild` |
| Any front-end stylesheet or build input (`.css`, `.html`, `.svg`) with no changed `.ts` or `.tsx` file | `format build` |

The plan must list each changed file that selected a row and the resulting
`checks` value. If no row maps a changed implementation file, stop and report
the unmapped file; do not use validation defaults. For a feature touching both
front-end TypeScript and Rust, the required value is
`checks="lint format tsc vitest coverage build clippy rusttest cargobuild"` when
a front-end test file changed, and the same value without `coverage` otherwise.

If validation fails, fix and re-run. Do not advance to Step 4 until `Skill: validate - output below` reports all checks PASS.

---

### Rework Routing

Whenever Steps 3–9 require a return to implementation, classify the required
production change before editing it. If it changes non-trivial logic or an
observable behavior covered by the TDD Provenance Gate, return to Step 2 and
obtain refreshed `Skill: testing-pro - output below` evidence before changing
production code. Otherwise, return to Step 3. Re-run every downstream step
whose evidence the rework invalidates; a prior passing artifact does not cover
changed production behavior.

---

### Step 4 — UI Verification (conditional)

**Trigger:** implementation touched a visual UI or interaction surface.
**Skip:** no visual UI or interaction surface changed.

Manually verify the changed UI: run the app, exercise the affected states, and
confirm they match the specification and the conventions in
`.claude/conventions/react-tauri/`. Emit a visible `Manual UI verification
record` in this form:

`Status` — exactly one of `Pass`, `Fail`, or `External verification limitation`.

**Environment** — macOS version, Safari/WebKit version when WKWebView-sensitive behavior is
verified, and app build mode.

| State / interaction | Expected result | Observed result | Result |
| --- | --- | --- | --- |

Each `Result` is exactly `Pass`, `Fail`, or `Not assessed`. A `Pass` status
requires every row to be `Pass`; a `Fail` status requires at least one `Fail`
row and names the implementation defect. `External verification limitation` is
allowed only when no implementation defect was found and must add this table:

| Scope | Cause | Unavailable Coverage | Implementation Defect Found |
| --- | --- | --- | --- |

The final column must be `no`. This table uses the same limitation fields that
`test-runner` requires.

On `Fail`, use Rework Routing. On `External verification limitation`, pass the
record to Step 5. If the manual verification cannot produce one of these
statuses, stop and report the blocker.

---

### Step 5 — Dedicated Validation

Agent: `.claude/agents/test-runner.md`

Required output: `Agent: test-runner - output below`

The agent runs the `Touched-layer validation plan` commands and the applicable
manual checks. If validation fails, use Rework Routing. If validation is
`Blocked`, continue only when its artifact explicitly identifies an external
verification limitation; otherwise stop and report the blocker. Record a
permitted limitation and continue to Step 6.

For UI/interaction changes, the agent consumes the Step 4 manual UI verification
record. Pass the visible `Manual UI verification record` as explicit input. It
requires a passing result unless that record explicitly identifies an external
verification limitation, which it records without treating as a fail.

---

### Step 6 — Design Self-Review (conditional)

**Trigger:** implementation touched a visual UI or interaction surface.
**Skip:** no visual UI or interaction surface changed.

Self-review the visual result against the `.claude/conventions/react-tauri/`
accessibility, performance, and platform-scope conventions. Emit a visible
`Design self-review record` in this form:

`Status` — exactly one of `Pass`, `Needs revision`, or `External verification limitation`.

| Convention / requirement | Evidence checked | Result |
| --- | --- | --- |

Each `Result` is exactly `Pass`, `Needs revision`, or `Not assessed`. A `Pass`
status requires every row to be `Pass`; `Needs revision` requires at least one
row with that result. `External verification limitation` is allowed only when
no implementation defect was found and must add the same `Scope`, `Cause`,
`Unavailable Coverage`, and `Implementation Defect Found` table defined in
Step 4, with `no` in its final column.

On `Needs revision`, use Rework Routing. On `External verification limitation`,
record the limitation and continue to Step 7. If the review cannot produce one
of these statuses, stop and report the blocker. If skipped, state `Skipped — no
visual UI or interaction surface` in the closure record.

---

### Step 7 — Code Review

Agent: `.claude/agents/code-reviewer.md`
Required output: `Agent: code-reviewer - output below`

If verdict is `Needs revision` or `Blocked` because TDD provenance is missing or invalid, return to Step 2. If verdict is `Needs revision` for another finding, use Rework Routing. For any other `Blocked` verdict, stop and report the blocker.
Do not advance to Step 8 until verdict is `Approved` or `Approved with minor notes`.

---

### Step 8 — Documentation Maintenance (conditional)

**Trigger:** implementation changes an authoritative documentation fact:
observable behavior, a public interface, a command signature, an architecture
constraint, or a documented domain rule.
**Skip:** the implementation is internal-only and changes none of those facts.

Skill: `.claude/skills/documentation-maintenance/SKILL.md`
Required output: `Skill: documentation-maintenance - output below`

If triggered, do not advance to Step 9 until this artifact is present. If
skipped, record `Skipped — no authoritative documentation fact changed` in the
closure record.

---

### Step 9 — Definition of Done (DoD) Gate

Run the DoD quality gate to verify all acceptance criteria pass, smoke checklist is complete, and edge cases are covered.

Skill: `.claude/skills/task-quality/SKILL.md`

Required input: existing TaskPilot ID from `Manager: manager - output below`
Required output: `Skill: task-quality - output below`

If the verdict is `blocked`, fix gaps and re-run. Do not advance to Step 10 until the quality gate reports `pass`.

---

### Step 10 — Local Commit And TaskPilot Completion

After the DoD gate passes, invoke `.claude/skills/taskpilot-work/SKILL.md` to
add completion evidence and perform the verified `in_progress → done`
transition. Its artifact must prove that reloading the item returned `done`.
Then, following the commit boundary already established by
`.claude/skills/work-with-git/SKILL.md`, stage the finalized TaskPilot records
with the task code and create the one local task-scoped commit required by
`AGENTS.md`; do not push. Emit visible `Local commit evidence` naming the
commit hash and any uncommitted remainder.

Required evidence: `Skill: taskpilot-work - output below` and `Local commit
evidence`

If the local commit fails after the verified `done` transition, follow the
commit-failure recovery procedure in `.claude/skills/work-with-git/SKILL.md`.

For a declared delivery cohort, perform its one shared local commit and complete
both task records atomically only after both DoD gates pass. If either cannot
complete, split the cohort or leave both items unfinished.

Do not advance to Task Complete without both lifecycle and commit artifacts.

---

### Step 11 — Task Complete

Skill: `.claude/skills/task-complete/SKILL.md`
Required output: `Skill: task-complete - output below`
