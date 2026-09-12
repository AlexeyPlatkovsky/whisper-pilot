# WhisperPilot agent contract

## Project

WhisperPilot is a macOS app for local and optional BYOK transcription workflows. Use [docs/idea.md](docs/idea.md) for
product scope and [docs/architecture.md](docs/architecture.md) for technical boundaries.

## Authority

- Answer questions, assessments, and reviews before making requested changes.
- Preserve user-authored changes. Never discard or rewrite them without explicit approval.
- Ask before destructive actions, external writes, dependency additions, or permission changes.
- Do not create, switch, merge, delete, publish, or push a branch without explicit approval.
- Commit only when the user explicitly asks. A push always needs a current explicit request.
- Keep private media, transcripts, credentials, and local settings out of commits and reports.

## Work policy

- Trivial, local, reversible work may proceed directly.
- Non-trivial product or product-documentation work requires an existing TaskPilot item.
- An explicit no-TaskPilot request waives TaskPilot workflow only for that request.
- AI-landscape maintenance is TaskPilot-exempt unless the user asks to track it.
- Resolve unclear product behavior before implementation; do not invent requirements.
- Add a focused failing test before changing non-trivial production logic.
- Finish requested changes by running affected checks and reviewing the final diff.
- Use an independent reviewer for non-trivial code and instruction-system changes.
- Check authoritative documentation after behavior, architecture, command, or UI changes.
- When committing tracked work, include its TaskPilot record in the same task-scoped commit.
- Significant transcription changes require an explicit real-Metal macOS validation result.

## Agents

Use a subagent only when fresh context materially helps: independent review, isolated TDD test authoring, noisy
validation, visual comparison, or large read-heavy exploration. Keep write-heavy agents sequential and give every agent
an exact input and mutation boundary. Project agents are discovered from [.codex/agents](.codex/agents).

## Skills

Codex discovers project skills from [.agents/skills](.agents/skills). Load only the skill whose description matches the
current task.

| Need                                 | Capability                  |
| ------------------------------------ | --------------------------- |
| TaskPilot records and lifecycle      | `taskpilot-work`            |
| Requirements discovery               | `requirements-discovery`    |
| TDD and test selection               | `testing`                   |
| Documentation alignment              | `documentation-maintenance` |
| Pencil design or design/code sync    | `pencil-design`             |
| SDD document maintenance             | `sdd-docs`                  |
| Unknown bug cause                    | `bug-triage`                |
| Open decision with real alternatives | `brainstorm`                |

## Project references

- [docs/INDEX.md](docs/INDEX.md): documentation and decision map.
- [docs/design.md](docs/design.md): UX flows, screens, and states.
- [docs/testing.md](docs/testing.md): test strategy and project quality expectations.
- [docs/development.md](docs/development.md): setup and supported commands.
- [.claude/conventions/react-tauri](.claude/conventions/react-tauri): focused runtime rules.
- [.claude/conventions/sdd-doc-set.md](.claude/conventions/sdd-doc-set.md): SDD ownership.

## Deterministic checks

Run `npm run lint:ai-instructions` after changing the active landscape. Active instruction files must have at most 100
lines and 120 characters per line. Product commits use the version workflow in `scripts/bump-version.sh`.
AI-landscape-only commits do not change the product version.
