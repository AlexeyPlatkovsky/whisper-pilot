---
name: fix-bug
description: Ordered execution path for fixing a confirmed bug in WhisperPilot — test, fix, validate, review, document, and close.
---

# Pipeline: fix-bug

## Purpose

Sequence the steps for fixing a confirmed, root-caused bug: author the required test evidence, implement the minimal fix, validate, review, and close.

## Preconditions

Before this pipeline begins, all of the following must be present in the conversation:
- The manager has classified the task as non-trivial and selected this pipeline.
- `Manager: manager - output below` is present. If it is absent, stop and report: "Manager routing artifact is missing. Complete the AGENTS.md classification gate before entering this pipeline."
- An existing TaskPilot ID in `ready` status (or an explicitly approved resumed
  `blocked` item) is present in the manager artifact.
- `Skill: work-with-git - output below` reports the completed branch decision.
- A triage report (`Skill: triage-bug - output below`) containing all required output-contract fields, including `Log evidence`, OR the user has explicitly provided:
  - reproduction steps
  - root cause location (file + line range)
  - severity and affected layer(s)
- If the manager assigned **Lite** tier, the reduced-readiness confirmation line — `(reproduction or target identified) and DoD present` — must be present in the manager artifact (per `AGENTS.md` §Quality Tiers / `.claude/skills/task-routing/SKILL.md` §Output Contract). If it is absent, stop and report it.

If any precondition is missing, stop and report which item is absent. Do not proceed to Step 0.

## Steps

### Step 0 — Activate TaskPilot Item

Skill: `.claude/skills/taskpilot-work/SKILL.md`

Before test or production edits, perform the verified `ready → in_progress`
operation. The artifact must identify the active branch and, for batch work,
confirm this is the only active child unless a predeclared two-task delivery
cohort applies. Do not advance without the reloaded `in_progress` evidence.

---

### Step 1 — Test Authoring And Red Evidence

Skill: `.claude/skills/testing-pro/SKILL.md`
Required output: `Skill: testing-pro - output below`

For non-trivial logic, apply `.claude/skills/testing-pro/SKILL.md` through the skill.

Do not advance to Step 2 until this artifact is present.

---

### Step 2 — Fix

Implement the minimal code change that makes the failing test(s) pass without breaking existing tests.

Skill: `.claude/skills/implement-tauri-feature/SKILL.md`

Scope: implement the minimal code change that makes the failing test(s) pass without breaking existing tests.

Required output: `Skill: implement-tauri-feature - output below`

**Post-implementation validation** — before advancing to Step 3, run local CI-equivalent checks:

Skill: `.claude/skills/validate/SKILL.md`
Required output: `Skill: validate - output below`

Select checks matching the touched layers:
`checks="lint format tsc vitest coverage clippy rusttest cargobuild"`

If validation fails, fix and re-run. Do not advance until `Skill: validate - output below` reports all checks PASS.

---

### Step 3 — Runtime UI Verification (conditional)

**Trigger:** the fix changes a visual UI or interaction surface.
**Skip:** no visual UI or interaction surface changed.

Run the affected flow in the real Tauri macOS window and emit the `Manual UI
verification record` defined by `.claude/conventions/testing-taxonomy.md`
§Runtime UI verification evidence. On `Fail`, use Rework Routing. An external
verification limitation may continue only when the record states that no
implementation defect was found. Do not advance to Step 4 until the applicable
record is present.

---

### Step 4 — Dedicated Validation

Agent: `.claude/agents/test-runner.md`
Required output: `Agent: test-runner - output below`

The agent runs the applicable configured test suite for every touched layer
(`npm run test:run` for front-end; `cargo test --manifest-path
src-tauri/Cargo.toml` for Rust). For UI fixes, pass the Step 3 `Manual UI
verification record` as explicit input.
All tests — new and pre-existing — must pass. If validation fails, return to
Step 2 through Rework Routing.
Do not advance to Step 5 until this artifact is present with a passing result.

### Step 5 — Review

Agent: `.claude/agents/code-reviewer.md`
Required output: `Agent: code-reviewer - output below`

The reviewer must confirm:
- the TDD Provenance Gate evidence is complete and correctly applied
- the fix is minimal and does not introduce new behavior beyond the bug scope
- no Blocking or Major findings remain

If verdict is `Needs revision` or `Blocked` because TDD provenance is missing or invalid, use Rework Routing. If verdict is `Needs revision` for another finding, use Rework Routing. For any other `Blocked` verdict, stop and report the blocker.
Do not advance to Step 6 until verdict is `Approved` or `Approved with minor notes`.

---

### Step 6 — Documentation Maintenance (conditional)

**Trigger:** the fix changes an authoritative documentation fact: observable
behavior, a public interface, a command signature, an architecture constraint,
or a documented domain rule. Consult `AGENTS.md` to identify the owning source.
**Skip:** fix is internal-only with no externally visible behavioral change.

Skill: `.claude/skills/documentation-maintenance/SKILL.md`
Required output: `Skill: documentation-maintenance - output below`

Do not advance to Step 7 until this artifact is present (if triggered).

---

### Step 7 — Definition of Done (DoD) Gate

Run the DoD quality gate to verify all acceptance criteria pass, smoke checklist is complete, and edge cases are covered.

Skill: `.claude/skills/task-quality/SKILL.md`

Required input: existing TaskPilot ID from `Manager: manager - output below`
Required output: `Skill: task-quality - output below`

If the verdict is `blocked`, use Rework Routing. Do not advance to Step 8 until the quality gate reports `pass`.

---

### Step 8 — Local Commit And TaskPilot Completion

After DoD passes, invoke `taskpilot-work` to add completion evidence and perform
the verified `in_progress → done` transition. Then follow the commit boundary
and commit-failure recovery procedure in `.claude/skills/work-with-git/SKILL.md`;
do not push. The lifecycle artifact must prove reloaded `done` before the atomic
commit, and the closure record reports the commit hash. For a declared two-task
delivery cohort, both DoD gates must pass before both verified completion
transitions and their one shared commit.


### Step 9 — Task Complete

Skill: `.claude/skills/task-complete/SKILL.md`
Required output: `Skill: task-complete - output below`

---

### Rework Routing

Whenever Steps 2–7 require a return to implementation, classify the required
production change before editing it. If it changes non-trivial logic or an
observable behavior covered by the TDD Provenance Gate, return to Step 1 and
obtain refreshed `Skill: testing-pro - output below` evidence before changing
production code. Otherwise, return to Step 2. Re-run every downstream step
whose evidence the rework invalidates; a prior passing artifact does not cover
changed production behavior.
