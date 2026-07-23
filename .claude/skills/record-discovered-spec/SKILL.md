---
name: record-discovered-spec
description: Prepare a user-approved discover-feature specification as a canonical TaskPilot record update. Use after scope-verifier returns No gaps and the user explicitly approves the draft; the pipeline then delegates persistence to taskpilot-work.
---

# Skill: record-discovered-spec

## Required Inputs

- Existing TaskPilot item ID
- Final `Skill: discover-requirements - output below` artifact
- Final `Agent: scope-verifier - output below` artifact with verdict `No gaps`
- Explicit user approval from the current discovery run

If any input is absent, inconsistent, or identifies a different item, stop with `Status: blocked`. Do not reconstruct missing content from conversation memory.

## Procedure

1. Confirm that the supplied TaskPilot ID matches the routed work.
2. Convert the approved draft into the canonical item fields:
   - `title`: approved draft title
   - `description`: self-contained Description, Non-goals, Constraints / design notes, Dependencies / integration, Target surfaces, and proposed BDD scenarios or altitude-specific child breakdown
   - `dod`: approved Acceptance criteria (DoD), preserving checklist meaning as plain YAML list entries
3. Emit the prepared update for `.claude/skills/taskpilot-work/SKILL.md`:
   the item ID, approved title, description, `dod`, and the protected metadata
   that persistence must preserve. Do not mutate TaskPilot in this skill.

## Output Contract

Begin with:

`Skill: record-discovered-spec - output below`

Then emit:

| Status | TaskPilot Item | Prepared Fields | Persistence Handoff |
|--------|----------------|-----------------|------------|

Valid `Status` values: `completed` / `blocked`. Use `blocked` for an absent,
inconsistent, or mismatched required input; the caller must not continue until
a completed handoff is emitted.
For `completed`, `Persistence Handoff` must name `taskpilot-work` and include the
protected metadata to preserve.
