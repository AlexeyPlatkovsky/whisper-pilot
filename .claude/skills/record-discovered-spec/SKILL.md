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
- Manager Route run; reloaded item identity/type with status `in_progress`; the
  expected parent ID or explicit `none`; and one matching draft version/digest
  carried by draft, verifier, and approval

If any input is absent, inconsistent, or identifies a different item, stop with `Status: blocked`. Do not reconstruct missing content from conversation memory.

## Procedure

1. Confirm that TaskPilot ID/type matches the routed work and that draft,
   verifier, and approval digests match. Confirm reloaded status is
   `in_progress` and parent equals the manager's expected parent context.
2. Convert the approved draft into the canonical item fields:
   - `title`: approved draft title
   - `description`: self-contained Description, Non-goals, Constraints / design
     notes, Dependencies / integration, Target surfaces, proposed BDD scenarios,
     case-derivation evidence, Child-task breakdown, and Child-feature breakdown,
     preserving every altitude-required section and explicit not-applicable value
   - `dod`: approved Acceptance criteria (DoD), preserving checklist meaning as plain YAML list entries
3. Emit the prepared update for `.claude/skills/taskpilot-work/SKILL.md`:
   the item ID, approved title, description, `dod`, plus expected
   `in_progress` status and parent invariant. Do not obtain, emit, or preserve a
   live protected-metadata snapshot and do not mutate TaskPilot in this skill;
   both responsibilities belong to `taskpilot-work`.

## Output Contract

Begin with:

`Skill: record-discovered-spec - output below`

Then emit:

| Status | TaskPilot Item / Type | Draft Digest | Prepared Payload | Blocker |
|---|---|---|---|---|

Valid `Status` values: `completed` / `blocked`. Use `blocked` for an absent,
inconsistent, or mismatched required input; the caller must not continue until
a completed handoff is emitted.
For `completed`, emit exact title, description, and `dod` as fenced YAML and
validate field-by-field round-trip equivalence for every approved draft section,
including case derivation and both child breakdowns. Also emit expected status
`in_progress` and expected parent for consumer-side validation.
