---
name: task-complete
description: Closure reporting for non-trivial routed work in WhisperPilot.
---

# Skill: task-complete

## When This Skill Applies

Use at the end of non-trivial routed work when:
- the task ran through a manager-routed execution path
- the framework requires an explicit closure record

Do not use for:
- trivial tasks

The selected pipeline appends this skill as its final step for non-trivial routed work. For a manager-declared ad-hoc route with no pipeline, the route declaration sequences it after the direct capability. Execution skills do not invoke it directly.

For TaskPilot-tracked work, this skill is a **post-mutation** closure record:
it does not close the item itself. An implementation/fix closure requires the
final `Skill: taskpilot-work - output below` artifact proving a reloaded `done`
item. A discovery closure requires the corresponding artifact proving a reloaded
`ready` item. For the `AGENTS.md` AI-governance exemption, state that lifecycle
verification is not applicable and do not require a TaskPilot artifact.

## Rules

### 0. Required Inputs And Preflight

Require the exact manager artifact, task identity, selected pipeline or complete
manager-declared ad-hoc plan, and the route's `Route execution record`. Block
before closure when the manager artifact is missing/malformed, the route is
unresolved, route/task identity differs across inputs, or the authoritative
plan/evidence set is unavailable.

The execution record binds each attempt by `Route run + planned step/row ID +
artifact label (or handoff-invocation ID) + attempt number + SHA-256 of the
exact visible artifact text + terminal status`. Attempts start at `1` and
increase monotonically whenever rework invalidates the step. For closure, the
applicable artifact is the highest attempt for that exact key; block on a
missing, duplicate, non-monotonic, digest-mismatched, or differently bound
entry.

A skipped conditional row uses its stable step/row ID, the exact evaluated
skip condition, terminal status `skipped`, and `N/A` for artifact label,
attempt, and digest. If later rework makes that step applicable, invalidate the
skip row and record the executed artifact at attempt `1`.

### 1. Report Actual Execution

Report what happened, not an idealized plan. Make skipped or changed steps visible.

### 2. Required Format

After the output label, write `Closure: complete` or `Closure: blocked`. Then provide a markdown table with exactly these three columns — do not rename or add columns:

| Step | Skill / Agent | Comment |
|------|---------------|---------|

### 3. Every Executed Step Must Appear

Use the selected pipeline (or the manager-declared ad-hoc route) as the
authoritative plan. Every executed step is a row. If a planned conditional step
was skipped, include it and name the declared skip condition in `Comment`.
For a pipeline step that creates a visible manual record rather than invoking a
skill or agent, reference that record by its declared label.

### 4. Reference Output Artifacts

For every planned routed handoff, `Comment` must reference the step's visible
output artifact label, Route run/task identity, execution-record key, latest applicable attempt, and
accepted terminal status. For a conditional handoff that was skipped, reference
its declared skip condition instead. Do not infer an artifact from a tool call,
a commit, or an unlabelled narrative.

### 5. Refuse Incomplete Closure

If a required planned output artifact or visible manual record is missing from
the conversation, do not declare completion. Set `Closure: blocked` and name
the missing evidence so the manager can return to the missing step.

For tracked work, a passing DoD gate, a commit, or a completion comment is not
enough. If the required lifecycle artifact does not show the expected reloaded
TaskPilot state (`done` for implementation/fixes; `ready` for discovery), set
`Closure: blocked` and name the missing or mismatched state.

For tracked implementation/fix work, also require the route's successful
`Local commit evidence - output below` artifact with hash and task-scoped staged
paths. For TaskPilot-exempt AI-governance work, require the manager-declared
objective DoD with stable criterion IDs and one structural-validation evidence
row per criterion.

Any unplanned state-changing handoff blocks closure until the manager explicitly
re-routes it.

If `AGENTS.md` §Final Response Gate prevents completion because a required
artifact reports a failure or implementation blocker, set `Closure: blocked`
and name that artifact and state. A selected pipeline may close despite a
`Blocked` validation artifact only when its own step explicitly permits an
external verification limitation, the artifact records that limitation's scope
and cause, every independent downstream gate passed, and the closure table
names the limitation. No other `Blocked` artifact permits complete closure.

### 6. Final Response Requirement

For non-trivial routed work, the final response must include the `Skill: task-complete - output below` table. The task is not closed if the table appears only in an intermediate commentary message and is omitted from the final answer.

Apply `AGENTS.md` §Instruction System and §Final Response Gate for
final-response evidence. Include `Agent: test-runner - output below` only when
the selected route explicitly required that agent.

## Output Contract

Begin with:

`Skill: task-complete - output below`

Then write `Closure: complete` or `Closure: blocked`, followed by the closure table.
