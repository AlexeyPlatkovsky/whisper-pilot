---
name: documentation-maintenance
description: Post-change documentation maintenance after feature implementation, refactoring, or non-trivial bug fixes in WhisperPilot.
---

# Skill: documentation-maintenance

## When This Skill Applies

Run after:
- feature implementation that changes project behavior, interfaces, commands, architecture, or domain facts
- refactoring that changes project structure, ownership, public contracts, or documented workflows
- non-trivial bug fixes that change behavior, constraints, commands, or known failure modes

Do not run:
- for purely exploratory or discussion work
- for trivial edits with no user-visible, developer-visible, operational, or architectural effect
- for documentation-only tasks whose primary execution already updated the relevant docs

## Rules

### 1. Run After the Change

Inspect the actual diff or executed steps before deciding whether docs need maintenance. Do not predict updates before implementation is known.

### 2. Find Authoritative Doc Roots

Project authoritative documentation locations:
- `docs/idea.md` — primary design specification
- `docs/architecture.md` — implemented architecture; inspect/update only the sections affected by the change
- `README.md` — developer guide and documentation entry point

Check `AGENTS.md` for any additional registered doc roots added since this skill was written.

### 3. Decide Whether Docs Are Affected

Check whether the change affects:
- public behavior or user workflows
- developer workflows or commands
- architecture, ownership, or source layout
- domain vocabulary or business rules
- known limitations, risks, or failure modes

If none apply, report that no documentation change was needed.

### 4. Update Narrowly

When documentation updates are needed:
- edit only affected docs
- preserve the project's existing documentation style
- update indexes or cross-references affected by the change
- for implemented architecture changes, update the focused sections in `docs/architecture.md` rather than duplicating architecture facts in unrelated docs
- avoid broad rewrites unless the task explicitly requires them

If the needed update is unclear, risky, or outside the approved task scope, report the gap instead of guessing.

### 5. Report the Result

Report one of:
- documentation updated
- documentation checked, no update needed
- documentation update needed but blocked — name the affected area, why it cannot be updated safely, and what is needed

## Output Contract

Begin with:

`Skill: documentation-maintenance - output below`

| Status | Docs Checked | Result |
|--------|--------------|--------|

`Status` must be one of: `documentation updated`, `documentation checked, no update needed`, or `documentation update needed but blocked`.
