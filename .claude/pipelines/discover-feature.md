---
name: discover-feature
description: Elicit, verify, approve, and record requirements for a new or partially-defined feature, epic, or task before implementation begins.
---

# Pipeline: discover-feature

## Purpose

Drive requirements discovery through a structured Q&A loop with the user, verify completeness with a dedicated subagent, obtain explicit user approval, and record the approved spec in the canonical TaskPilot item. This pipeline runs **before** `implement-feature` — it produces the approved TaskPilot artifact that `implement-feature` consumes.

## Eligibility

Apply `.claude/skills/task-routing/SKILL.md` §Routing. This pipeline runs only when the manager routes requirements discovery or re-scoping here.

For a post-approval scope addendum, begin a new `discover-feature` run so the addendum receives its own verification and approval.

## Preconditions

Before this pipeline begins:
- The manager has classified the task as non-trivial and selected this pipeline.
- `Manager: manager - output below` artifact is present in the conversation.
- An existing TaskPilot ID in `backlog` status (or a user-approved resumed
  discovery item) is present in the manager artifact.
- `Skill: work-with-git - output below` reports the completed branch decision.

**If the `Manager: manager - output below` artifact is absent, stop immediately and return `Blocked` — do not begin Step 1.**

**If the TaskPilot ID or completed branch artifact is absent, stop immediately and return `Blocked` — do not begin Step 0.**

## Steps

---

### Step 0 — Start Discovery Lifecycle

Skill: `.claude/skills/taskpilot-work/SKILL.md`

Before Q&A, perform the verified `backlog → in_progress` discovery operation
and add the run/parent-context comment. Do not advance without the reloaded
`in_progress` evidence.

---

### Step 1 — Q&A (discover-requirements skill)

Skill: `.claude/skills/discover-requirements/SKILL.md`

The skill performs its own context loading (docs check and architecture reading) at the start of the session before the first round of questions.

On loop re-entry from Step 2, pass the current draft and the `scope-verifier` gap table to the skill and apply `.claude/skills/discover-requirements/SKILL.md` §Q&A Rounds.

Required output: `Skill: discover-requirements - output below` with a populated draft spec and the altitude-specific scenario and child-breakdown fields required by `.claude/skills/discover-requirements/SKILL.md` §Draft Spec Format.

Do not advance to Step 2 until this artifact is present and the draft spec includes all required fields.

---

### Step 2 — Scope verification (scope-verifier agent)

Agent: `.claude/agents/scope-verifier.md`

Pass the following as explicit structured input to the agent:
- The full `Skill: discover-requirements - output below` artifact from Step 1
- On loop re-entry: the prior `Agent: scope-verifier - output below` gap table as additional context

Required output: `Agent: scope-verifier - output below`

**If verdict is `Gaps found`:** return to Step 1. Pass the current draft and gap table as input. Do not advance.

**If verdict is `No gaps`:** advance to Step 3.

**If verdict is `Blocked`:** stop and report the agent's blocking reason. Do not synthesize a verdict in the parent session.

**If the required artifact is missing, malformed, or has any other verdict:** stop and report `Blocked` with the contract failure.

**Loop exit condition:** If scope-verifier has returned `Gaps found` three consecutive times and the gaps remain unresolved, pause the loop. Present the remaining gaps to the user with:

> "These gaps could not be resolved through Q&A. Please provide answers to the items below, or confirm explicitly that they are out of scope before proceeding."

Wait for user input, then re-enter Step 1 once with the current draft, remaining gap table, and the user's answers. The skill must emit a refreshed draft before the verifier runs again. Re-enter Step 2 once with that refreshed draft. If gaps still remain after this final verification pass, stop and report `Blocked`.

Apply `.claude/skills/agent-handoff/SKILL.md` for this handoff.

---

### Step 3 — User approval

Present the final draft spec to the user in a readable summary. State explicitly:

> "The scope-verifier found no gaps. Do you approve this spec to proceed?"

Wait for an explicit approval signal: "yes", "approved", "looks good", "go ahead", or equivalent direct confirmation. A non-committal response ("ok", "sure") does not count.

If the response is non-committal, re-ask once:

> "Please confirm with Yes or No: approve this spec and proceed?"

If the response remains non-committal after that single re-ask, stop and report `Blocked` — do not advance to Step 4.

Do not advance to Step 4 without explicit user approval.

---

### Step 4 — Prepare approved spec record

Skill: `.claude/skills/record-discovered-spec/SKILL.md`

Pass the TaskPilot item ID, the final `Skill: discover-requirements - output below` artifact, the `Agent: scope-verifier - output below` artifact with verdict `No gaps`, and the user's explicit approval.

Required output: `Skill: record-discovered-spec - output below` with `Status` = `completed`.

If the skill reports `blocked`, stop. Do not advance or treat the
conversation-only draft as implementation-ready.

---

### Step 5 — Persist approved spec

Skill: `.claude/skills/taskpilot-work/SKILL.md`

Pass the prepared record update from Step 4. The skill snapshots protected
metadata, updates the approved fields, runs TaskPilot validation, reloads the
item, and reports preservation evidence.

Required output: `Skill: taskpilot-work - output below` with `Operation result` = `completed`.

---

### Step 6 — Definition of Ready And Mark Ready

Skill: `.claude/skills/verify-readiness/SKILL.md`

Pass the persisted TaskPilot item and its parent context. Do not advance until
the result is `DoR gate: Ready`. If it cannot be resolved in the current run,
use `taskpilot-work` to record the blocker and transition the item to `blocked`.

After a Ready verdict, invoke `taskpilot-work` to perform the verified
`in_progress → ready` transition. Its output must prove the reloaded `ready`
item. A successful discovery run does not make the product item `done`.

---

### Step 7 — Task Complete

Run the route's final closure step after the discovery and persistence artifacts are present.

Skill: `.claude/skills/task-complete/SKILL.md`

Required closure output: `Skill: task-complete - output below`

Apply AGENTS.md §Final Response Gate before sending the final response.

The final response must include compact versions of:
- `Skill: discover-requirements - output below`
- `Agent: scope-verifier - output below`
- `Skill: record-discovered-spec - output below`
- `Skill: taskpilot-work - output below`
- `Skill: verify-readiness - output below`
- `Skill: task-complete - output below`
