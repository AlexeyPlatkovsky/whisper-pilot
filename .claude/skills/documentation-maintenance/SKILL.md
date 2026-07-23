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
- when the selected documentation-owning route already requires a different explicit
  documentation-validation artifact

## Rules

Require mode `implementation/fix` or `documentation-only verification`, the
manager Route run, maintenance-attempt number, approved scope, primary change
artifact, exhaustive pre-documentation changed-file list, and exact
pre-documentation task diff. For implementation/fix mode, the primary change
artifact is the implementation artifact; for documentation-only verification,
it is the routed documentation edit artifact. Start attempt at `1` and
increment after rework invalidates a prior result. Block before documentation
work when the change boundary cannot be established.

### 1. Run After the Change

Inspect the actual diff or executed steps before deciding whether docs need maintenance. Do not predict updates before implementation is known. For a documentation-only task, operate in **verification-only mode** after its primary edit: do not make a second documentation edit, but verify the authoritative source, affected indexes/cross-references, and command or fact accuracy.

### 2. Find Authoritative Doc Roots

Consult the authoritative-source table in `AGENTS.md` before deciding which
documentation is affected. Typical roots include `README.md` for user-facing
material, `docs/development.md` for developer workflow, and focused product or
architecture documents for their owned facts. Do not rely on a hard-coded,
possibly stale document list.

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
- update affected non-SDD indexes and cross-references; `docs/INDEX.md` belongs
  exclusively to `sdd-index-sync`
- for implemented architecture changes, update the focused sections in `docs/architecture.md` rather than duplicating architecture facts in unrelated docs
- avoid broad rewrites unless the task explicitly requires them

After any documentation edit, refresh the exhaustive changed-file list and
inspect the final task diff before reporting success. If an SDD document or
feature changed, perform the invoking pipeline's declared conditional
`sdd-index-sync` substep and require its `Status: completed` artifact before
success; this skill must not edit `docs/INDEX.md` itself.

If the needed update is unclear, risky, outside the approved task scope, or
explicitly deferred by the user, report the gap instead of guessing or expanding
the task. Do not turn an AI-governance-only change into product-documentation
work merely because a related product document exists.

### 5. Report the Result

Report one of:
- documentation updated
- documentation verified (documentation-only verification mode)
- documentation checked, no update needed
- documentation update needed but blocked — name the affected area, why it cannot be updated safely, and what is needed

## Output Contract

Begin with:

`Skill: documentation-maintenance - output below`

| Status | Diff / Scope | Authoritative Sources | Docs Checked / Changed | Validation | Index Disposition | Result |
|---|---|---|---|---|---|---|

Also emit the exact mode, manager Route run, and maintenance-attempt number.

`Status` must be one of: `documentation updated`, `documentation verified`,
`documentation checked, no update needed`, or `documentation update needed but blocked`.
Mode `implementation/fix` permits only `documentation updated`,
`documentation checked, no update needed`, or the blocked status. Mode
`documentation-only verification` permits only `documentation verified` or the
blocked status.
The blocked status fails the handoff. All success statuses require final-diff
inspection, a refreshed exhaustive changed-file list, path/link checks, an
authoritative source for each affected fact, and completed `sdd-index-sync`
evidence when applicable.
