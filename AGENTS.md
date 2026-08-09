# AGENTS.md — WhisperPilot Root Contract

This file is the root operational contract for the WhisperPilot project.
All AI tools working on this project must read this file before starting any
work. This file overrides any tool-specific adapter on conflict. It states
root invariants and routing triggers only; each referenced skill, agent, or
pipeline owns its own procedure.

## Agent And Subagent Execution

Distinct-agent handoffs must preserve the invoked agent's declared
state-mutation boundary and must not be executed inline;
`.claude/skills/agent-handoff/SKILL.md` owns runner selection, eligibility,
and recovery. An applicable project instruction, pipeline, or routed handoff
that explicitly names an agent or subagent is standing user authorization to
invoke that agent — do not request separate approval for the invocation.

---

## Project

WhisperPilot is a macOS desktop application for **offline transcription of
local audio and video files**, with speaker attribution and a local-LLM
summary. There is no live capture and no cloud. Product scope, milestones,
and engines are owned by `docs/idea.md`; technical architecture is owned by
`docs/architecture.md`. Do not restate them here.

---

## Question Before Action

When the user asks a question, requests an assessment or review, or asks
whether something is possible, answer the question before taking
implementation action. Read-only inspection is permitted when needed to
answer accurately; do not create, edit, or delete files, run state-changing
commands, or initiate work unless the user explicitly asks to proceed. If one
message contains both a question and an explicit implementation request,
answer the question briefly first, then follow classification and routing.

---

## Task Classification

Stating the task classification is a required output. Before any file is
created, edited, or deleted, state it explicitly in your response as
`<complexity>; <risk>; <domain>`. If that line is absent, the classification
gate has not been satisfied and no file may be touched. The classification
line uses the complexity/risk/domain vocabulary and trivial-boundary
definition in `.claude/skills/task-routing/SKILL.md` §Classification, even
when the work is trivial; that skill's gates still apply only to non-trivial
work.

- **Trivial work** — execute directly, invoking the applicable skills from
  the catalog in §Instruction System. No TaskPilot item is required.
- **Non-trivial work** — load `.claude/skills/task-routing/SKILL.md` and
  follow it; do not implement until it emits
  `Manager: manager - output below`. That skill owns the trivial/non-trivial
  boundary definition, risk selection, Quality Tier statement, and route
  selection.
- When unsure whether work is trivial, treat it as non-trivial.
- TaskPilot-only administration is exempt from routing;
  `.claude/skills/taskpilot-work/SKILL.md` owns that procedure. The exemption
  covers metadata-only administration; the underlying non-trivial work still
  requires its TaskPilot identity and branch decision.

---

## Non-Negotiable Gates And User Waivers

The classification gate, the `task-routing` gate for non-trivial work
(subject only to the trivial fork and routing exemption in §Task
Classification), the Git
branch decision gate for non-trivial work, and the system-level approval stop
are mandatory stop gates. They may not be skipped, deferred, or collapsed for
convenience. For trivial work the branch gate resolves by defaulting to the
current branch per §Git Operation Authority.

A user waiver is interpreted literally and minimally: it removes only the
specific obligation the user named, and nothing adjacent. A terse instruction
("just fix it", "no ticket", "quick change") waives nothing; treat a broad
instruction as authorization to do the work through the gates, not permission
to skip them. To skip a mandatory gate the user must name that specific gate.
An operating mode that biases toward action (for example, Auto Mode) changes
only whether to ask optional clarifying questions; when bias-to-action and a
stop gate conflict, the stop gate wins.

---

## Task Identity And Tracking

TaskPilot (project key **WP**) is the source of truth for non-trivial product
work and product/documentation maintenance; an existing TaskPilot item is
required before implementation for that work.
`.claude/skills/taskpilot-work/SKILL.md` owns item lookup, approval, record
structure, commands, and lifecycle procedure.

**AI-governance maintenance is TaskPilot-exempt.** The exemption covers this
root `AGENTS.md` and the operational materials under `.claude/` when the
change governs AI execution rather than product behavior or product
documentation; it never exempts `docs/`, feature specifications, or any
product/runtime artifact. Exempt work still requires classification, the
routed quality gates, the Git-operation decision, and closure evidence.
**TaskPilot-exempt is not the Quality Tier `Exempt`:** non-trivial
AI-governance work remains Full tier.

### Git Operation Authority

Do not create, switch to, publish, delete, merge, or otherwise mutate a Git
branch without the user's explicit approval; approval of a TaskPilot item is
never approval of a Git operation. Without branch approval, trivial work
remains on the current branch; for non-trivial work, stop and ask before
editing. Never discard, overwrite, or history-rewrite uncommitted user
changes; any destructive Git operation requires explicit user approval. User
approval to create a new task branch also authorizes exactly one immediate
`git push -u origin <branch>` after that branch is actually created,
establishing the matching `origin/<branch>` upstream. That initial publication
does not authorize any subsequent push.
`.claude/skills/work-with-git/SKILL.md` owns branch selection and reporting.

---

## Quality Tiers

- **Full** — all non-trivial product, engineering, instruction-system,
  high-risk, or system-level work. Apply every routed quality gate.
- **Lite** — low/medium-risk non-trivial reference documentation or
  visual-only work with no runtime behavior change. Require a defined target
  and Definition of Done (the Lite readiness confirmation), the routed git
  gate, relevant validation, and task-complete.
- **Exempt** — trivial work only. Trivial fixes and changes need no TaskPilot
  item.
- When work qualifies for more than one tier, use the higher tier.

---

## Quality Gates

These apply to all non-trivial work and may not be skipped:

- Non-trivial logic must satisfy the TDD provenance gate in
  `.claude/skills/testing-pro/SKILL.md` before production edits.
- UI changes require the manual verification selected by
  `.claude/skills/validate/SKILL.md`.
- Documentation maintenance
  (`.claude/skills/documentation-maintenance/SKILL.md`), local validation
  (`.claude/skills/validate/SKILL.md`), and DoD closure
  (`.claude/skills/task-quality/SKILL.md`) must use their routed owning
  skills.

---

## Commit And Push Boundary

- Except for the one initial publication authorized by §Git Operation
  Authority for an actually newly created task branch, never push unless the
  user explicitly requests a push in the current instruction. Pipeline
  execution, review cycles, validation runs, and task completion never
  authorize a push.
- A tracked task's code and its related TaskPilot records belong in one
  atomic task-scoped local commit. AI-governance maintenance commits only on
  an explicit user request.
- `.claude/skills/work-with-git/SKILL.md` owns commit composition, message
  format, staging verification, and failure recovery.

---

## Final Response Gate

For non-trivial work, use `.claude/skills/task-complete/SKILL.md` to report
the selected route's required evidence. A failed check, missing requirement,
or unsafe/unimplemented behavior blocks completion. An external
post-implementation check that is explicitly documented as a verification
limitation is not missing evidence.

---

## Instruction System

The instruction system is a curated engineering core plus the Spec-Driven
Development (SDD) tooling. Its capabilities live under `.claude/skills/`,
`.claude/agents/`, `.claude/pipelines/`, `.claude/conventions/`, and
`.claude/sdd/` (doc templates); `.claude/skills/task-routing/SKILL.md` is the
authoritative router and lists every available route. Material
instruction-system changes are themselves non-trivial and route through
`task-routing`. The tables below list each capability and its trigger only;
each entry's own file is its sole behavioral authority.

### Skills

| Skill | Trigger |
|---|---|
| `agent-handoff` | A routed handoff to a distinct agent or subagent needs a runner selected or recovered |
| `brainstorm` | An open design decision with meaningful trade-offs needs structured discussion |
| `design-in-pen` | A UI design mockup needs creating or iterating on in `pencil/*.pen`, before implementation |
| `discover-requirements` | Feature, epic, or task scope is unclear and requirements must be elicited before work |
| `documentation-maintenance` | A feature, refactor, or non-trivial bug fix has landed and documentation may be stale |
| `implement-tauri-feature` | Routed feature or confirmed bug-fix scope is ready to implement after its gates |
| `react-tauri-expert` | A React/TypeScript/Tauri approach or convention question needs read-only advice |
| `record-discovered-spec` | A user-approved discovery specification must become a canonical TaskPilot record |
| `sdd-doc-author` | One SDD main or extension document must be created or updated |
| `sdd-feature-author` | One SDD feature folder must be scaffolded or updated |
| `sdd-index-sync` | A doc, feature, or ADR change requires rebuilding `docs/INDEX.md` |
| `sync-pen-code` | `pencil/*.pen` needs syncing with the real UI code, in either direction |
| `task-complete` | Non-trivial routed work is ready for closure reporting |
| `task-quality` | Completion evidence must be mapped against the Definition of Done before closure |
| `task-routing` | Work is (or may be) non-trivial and needs classification and a route |
| `taskpilot-work` | A TaskPilot item needs lookup, creation, field changes, lifecycle, or validation |
| `testing-pro` | Vitest or Rust test code must be written or improved |
| `triage-bug` | A reported bug or unexpected behavior has an unknown root cause |
| `validate` | The working tree needs local CI-equivalent checks with a pass/fail report |
| `verify-readiness` | A routed item needs its Definition-of-Ready check before implementation |
| `work-with-git` | A routed task needs its branch decision or commit/push boundary applied |

### Agents

| Agent | Trigger |
|---|---|
| `code-reviewer` | A completed, validated implementation diff needs review before closure |
| `instruction-evaluator` | A changed AI-governance instruction artifact needs isolated review |
| `pencil-vision-reviewer` | A `pencil/*.pen` mutation (via the pencil MCP or the `pen` CLI) needs its rendered result checked against design intent or a counterpart image |
| `scope-verifier` | A draft requirements specification needs a structural completeness check |
| `sdd-gap-analyzer` | SDD adoption or expansion needs a docs-versus-code gap inventory |
| `sdd-spec-reviewer` | An SDD docs tree needs a completeness and traceability review |
| `test-runner` | Routed work needs build, test, or manual validation executed and reported |

---

## Version Management

Before every commit, run `scripts/bump-version.sh` with the appropriate bump
type for the work being committed:

| Bump type | When to use |
|---|---|
| `patch` | Bug fix, refactor, chore, or any change that does not add a user-facing feature |
| `major` | New task, feature, or epic that adds user-facing functionality |
| `release` | Only when the user explicitly asks for a release version bump |

The script updates the version in both `src-tauri/Cargo.toml` and
`src-tauri/tauri.conf.json`. Run it before staging; the version change becomes
part of the same commit as the feature or fix it describes. If multiple
uncommitted changes of different bump types are present, use the highest
applicable bump type among them.

---

## Spec-Driven Development

The project adopts the **Standard** SDD tier. The authoritative spec lives in
`docs/` and is mapped by `docs/INDEX.md`. The `.claude/conventions/sdd-doc-set.md`
convention defines document ownership, tiers, the feature-folder schema, the ID
scheme, and the traceability spine. Keep `docs/INDEX.md` in sync after any
doc/feature change via `.claude/skills/sdd-index-sync/SKILL.md`.

---

## Authoritative Sources

| Source | Purpose |
|---|---|
| `docs/INDEX.md` | Live map of the SDD docs tree, feature registry, and decision log |
| `docs/idea.md` | Problem, users, value, scope, non-goals, principles, success signals |
| `docs/architecture.md` | Technical architecture — layer map, pipeline, IPC contract, models, build |
| `docs/design.md` | Product/UX design — flows, screens, states |
| `docs/testing.md` | Test strategy, levels, quality gates |
| `docs/roadmap.md` | Milestones M1–M3, sequencing, non-goals over time |
| `docs/decisions/` | ADRs — rationale for the key decisions |
| `docs/features/F*/` | Per-feature requirements, tasks (TaskPilot-linked), scenarios |
| `README.md` | User-facing overview — description, released features, requirements, how to run |
| `docs/development.md` | Developer guide — prerequisites, build/dev/test commands, layout |
| `src-tauri/src/lib.rs` | Tauri core — command registration, app state, module map |
| `src-tauri/src/main.rs` | Tauri/Rust entry point |
| `src/App.tsx` | React UI root |
