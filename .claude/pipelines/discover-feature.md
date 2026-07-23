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
- `Manager: manager - output below` artifact with a stable Route run is present
  in the conversation.
- An existing TaskPilot ID and exact start mode are present: initial
  `backlog`, explicitly resumed `blocked`, continued `in_progress`, or a
  `ready` item entering approved re-scope/addendum discovery.
- `Skill: work-with-git - output below` reports the completed branch decision.

**If the `Manager: manager - output below` artifact is absent, stop immediately and return `Blocked` — do not begin Step 1.**

**If the TaskPilot ID or completed branch artifact is absent, stop immediately and return `Blocked` — do not begin Step 0.**

## Execution Record

Maintain a `Route execution record` bound to the manager Route run. For every
planned step/conditional row, record its stable step ID, visible artifact label
or declared skip, attempt (starting at `1`, incremented after invalidating
rework), SHA-256 of exact artifact text, and terminal status.
For a declared conditional skip, record the exact evaluated condition and
status `skipped`, with artifact label, attempt, and digest `N/A`.

## Steps

---

### Step 0 — Start Discovery Lifecycle

Skill: `.claude/skills/taskpilot-work/SKILL.md`

Before Q&A, perform the mode-specific verified operation: `backlog →
in_progress`, approved `blocked → in_progress`, verified continued
`in_progress`, or `ready → in_progress` for approved re-scope/addendum.
and add the run/parent-context comment. Do not advance without the reloaded
`in_progress` evidence.

---

### Step 1 — Q&A (discover-requirements skill)

Skill: `.claude/skills/discover-requirements/SKILL.md`

The skill performs its own context loading (docs check and architecture reading) at the start of the session before the first round of questions.
Pass the manager artifact's exact Route run and routed item type as explicit
inputs; the draft version must reuse that Route run.

On loop re-entry from Step 2, pass the current draft and the `scope-verifier` gap table to the skill and apply `.claude/skills/discover-requirements/SKILL.md` §Q&A Rounds.

Required output: `Skill: discover-requirements - output below` with a populated
draft spec, its computed version/digest, and the altitude-specific scenario and
child-breakdown fields required by `.claude/skills/discover-requirements/SKILL.md`
§Draft Spec Format.

Do not advance to Step 2 until this artifact is present and the draft spec
includes all required fields. If it reports `Status: Blocked`, apply the
post-activation blocker rule and stop.

---

### Step 2 — Scope verification (scope-verifier agent)

Agent: `.claude/agents/scope-verifier.md`

Pass the following as explicit structured input to the agent:
- The full `Skill: discover-requirements - output below` artifact from Step 1
- Its declared draft version and SHA-256 digest for independent recomputation
- Invocation mode: `initial` on the first pass, `gap-re-entry` after `Gaps
  found`, or `approval-revision` after the user rejects/changes a verified draft
- On either revision: the complete prior versioned draft and prior
  mode-appropriate `Agent: scope-verifier - output below` artifact as context

Required output: `Agent: scope-verifier - output below`
Required handoff evidence: `Skill: agent-handoff - output below`

**If verdict is `Gaps found`:** return to Step 1. Pass the current draft and gap table as input. Do not advance.

**If verdict is `No gaps`:** advance to Step 3.

**If verdict is `Blocked`:** stop and report the agent's blocking reason. Do not synthesize a verdict in the parent session.

**If the required artifact is missing, malformed, or has any other verdict:** stop and report `Blocked` with the contract failure.

Give each gap a stable ID. Count a consecutive pass only while the same gap ID
remains unresolved; resolved IDs leave the set, new IDs use
`max(previous numeric ID)+1`, and each new gap's pass count starts at one. After
the same gap reaches three consecutive passes, pause the loop and present the
remaining gaps to the user with:

> "These gaps could not be resolved through Q&A. Please provide answers to the items below, or confirm explicitly that they are out of scope before proceeding."

Wait for user input, then re-enter Step 1 once with the current draft, remaining gap table, and the user's answers. The skill must emit a refreshed draft before the verifier runs again. Re-enter Step 2 once with that refreshed draft. If gaps still remain after this final verification pass, stop and report `Blocked`.

Apply `.claude/skills/agent-handoff/SKILL.md` for this handoff.

---

### Step 3 — User approval

Reuse the exact version/digest verified in Step 2. Present the verified draft to the user and
state explicitly:

> "The scope-verifier found no gaps. Do you approve this spec to proceed?"

Wait for an explicit approval signal: "yes", "approved", "looks good", "go ahead", or equivalent direct confirmation. A non-committal response ("ok", "sure") does not count.

If the response is non-committal, re-ask once:

> "Please confirm with Yes or No: approve this spec and proceed?"

If the response is `no`, conditional, or changes scope, return to Step 1 in
approval-revision mode with the prior verified draft and `No gaps` artifact,
then fully re-verify the new draft version. If it remains non-committal after one re-ask,
stop and report `Blocked`.

Emit `User approval record` containing the draft digest/version and verbatim
disposition. Do not advance unless the approval record, verifier artifact, and
draft identify the same version.

---

### Step 4 — Prepare approved spec record

Skill: `.claude/skills/record-discovered-spec/SKILL.md`

Pass the manager Route run, TaskPilot item ID, final versioned draft, matching
verifier artifact, matching `User approval record`, and reloaded item
identity/type/status/parent plus the manager's expected parent context.

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

Pass the persisted TaskPilot item, manager route, approval provenance, and
parent context. Do not advance until
the result is `DoR gate: Ready`. If it cannot be resolved in the current run,
apply each disposition: `create` returns to the manager; `skip` uses
`taskpilot-work` to persist the user-approved narrower scope; `ignore` applies
only to a separately reported non-gating artifact. Re-run readiness after any
mutation. If unresolved, use `taskpilot-work` to record the blocker and
transition the item to `blocked`.

After a Ready verdict, invoke `taskpilot-work` to perform the verified
`in_progress → ready` transition. Its output must prove the reloaded `ready`
item. A successful discovery run does not make the product item `done`.

---

### Post-activation blocker rule

After Step 0, every unresolved stop—including verifier failure, exhausted gaps,
approval failure, preparation/persistence failure, or readiness failure—must
invoke `taskpilot-work` to record cause, evidence, and exact unblocking action
and reload-verify `blocked`.

---

### Step 7 — Task Complete

Run the route's final closure step after the discovery and persistence artifacts are present.

Skill: `.claude/skills/task-complete/SKILL.md`

Required closure output: `Skill: task-complete - output below`

Apply AGENTS.md §Final Response Gate before sending the final response.

The final response must include compact versions of:
- `Skill: discover-requirements - output below`
- `Agent: scope-verifier - output below`
- `Skill: agent-handoff - output below`
- `User approval record`
- `Skill: record-discovered-spec - output below`
- `Skill: taskpilot-work - output below`
- `Skill: verify-readiness - output below`
- `Skill: task-complete - output below`
