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
- isolated single-step low-risk work
- trivial cosmetic changes

The selected pipeline appends this skill as its final step for non-trivial routed work. For a manager-declared ad-hoc route with no pipeline, the route declaration sequences it after the direct capability. Execution skills do not invoke it directly.

For TaskPilot-tracked work, this skill is a **post-mutation** closure record:
it does not close the item itself. An implementation/fix closure requires the
final `Skill: taskpilot-work - output below` artifact proving a reloaded `done`
item. A discovery closure requires the corresponding artifact proving a reloaded
`ready` item. For the `AGENTS.md` AI-governance exemption, state that lifecycle
verification is not applicable and do not require a TaskPilot artifact.

## Rules

### 1. Report Actual Execution

Report what happened, not an idealized plan. Make skipped or changed steps visible.

### 2. Required Format

After the output label, write `Closure: complete` or `Closure: blocked`. Then provide a markdown table with exactly these three columns — do not rename or add columns:

| Step | Skill / Agent | Comment |
|------|---------------|---------|

### 3. Every Executed Step Must Appear

Every executed step is a row. If a planned step was skipped, include it and explain why in `Comment`.

### 4. Reference Output Artifacts

For planned routed handoffs, `Comment` must reference the step's visible output artifact label (e.g. `Skill: implement-tauri-feature - output below`).

### 5. Refuse Incomplete Closure

If a required planned output artifact is missing from the conversation, do not declare completion. Set `Closure: blocked` and name the missing artifact so the manager can return to the missing step.

For tracked work, a passing DoD gate, a commit, or a completion comment is not
enough. If the required lifecycle artifact does not show the expected reloaded
TaskPilot state (`done` for implementation/fixes; `ready` for discovery), set
`Closure: blocked` and name the missing or mismatched state.

If `AGENTS.md` §Final Response Gate prevents completion because a required artifact
reports a failure or implementation blocker, set `Closure: blocked` and name that
artifact and state. A selected pipeline may close with a `Blocked` artifact only
when it explicitly records an external verification limitation, every independent
downstream gate ran successfully, and the closure table names its scope and cause.

### 6. Final Response Requirement

For non-trivial routed work, the final response must include the `Skill: task-complete - output below` table. The task is not closed if the table appears only in an intermediate commentary message and is omitted from the final answer.

Apply `AGENTS.md` §Instruction System Changes and §Final Response Gate for final-response evidence. Include `Agent: test-runner - output below` only when the selected route explicitly required that agent.

## Output Contract

Begin with:

`Skill: task-complete - output below`

Then write `Closure: complete` or `Closure: blocked`, followed by the closure table.
