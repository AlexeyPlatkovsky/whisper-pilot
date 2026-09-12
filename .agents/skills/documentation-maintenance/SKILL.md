---
name: documentation-maintenance
description: Align authoritative WhisperPilot documentation after behavior, architecture, command, or UI changes.
---

# Documentation maintenance

## Apply when

Use after a change affects user behavior, developer commands, architecture, domain rules, or documented UI. Skip when
the final diff has no documented or user-visible effect and state that reason.

## Sources

- `docs/idea.md`: product scope and principles.
- `docs/architecture.md`: technical boundaries and ownership.
- `docs/design.md`: UX flows, screens, states, and copy.
- `docs/testing.md`: test strategy and quality policy.
- `docs/roadmap.md`: milestones and sequencing.
- `docs/development.md`: developer setup and commands.
- `README.md`: user-facing overview and setup.
- `pencil/*.pen`: visual source when it mirrors shipped UI.

## Policy

- Inspect the final diff before deciding what documentation changed.
- Update only the authoritative owner of each affected fact; link instead of duplicating.
- Verify commands and code references against the repository.
- Use `pencil-design` for `.pen` mutation; never edit its JSON directly.
- Use `sdd-docs` when an SDD document or `docs/INDEX.md` changes.
- Do not add speculative roadmap promises or implementation history.

## Result

Report updated paths and facts, or the exact reason no documentation update was needed.
