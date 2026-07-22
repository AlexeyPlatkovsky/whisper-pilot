# AGENTS.md — WhisperPilot Root Contract

This file is the root operational contract for the WhisperPilot project.
All AI tools working on this project must read this file before starting any work.
This file overrides any tool-specific adapter on conflict.

Instruction authority is layered: `AGENTS.md` owns project-wide policy;
`task-routing` owns classification and routing; pipelines own step order; skills
and agents own procedures and output contracts; and conventions own shared
quality standards. Lower layers may narrow a higher-layer rule but must not
contradict it.

## Agent And Subagent Execution

Distinct-agent handoffs must preserve the invoked agent's declared tool access,
sandbox, and state-mutation boundary; they must not be executed inline. When
runner selection or recovery is necessary, use
`.claude/skills/agent-handoff/SKILL.md`. An agent already running in an isolated
execution context executes its own instructions directly.

---

## Project

WhisperPilot is a macOS desktop application for **offline transcription of local
audio and video files**, primarily in Russian, with speaker attribution and a
local-LLM summary. There is no live capture and no cloud. Product scope,
milestones, and engines are owned by `docs/idea.md`; technical architecture is
owned by `docs/architecture.md`. Do not restate them here.

---

## Question Before Action

When the user asks a question, requests an assessment or review, or asks whether
something is possible, answer the question before taking implementation action.
Do not interpret questions such as "can we do this?", "is this correct?", or
"would this work?" as requests to make changes.

Read-only inspection is permitted when needed to answer accurately. Do not
create, edit, or delete files; run state-changing commands; or initiate work
unless the user explicitly asks to implement, modify, run, execute, or otherwise
proceed with the action.

If the user asks both a question and explicitly requests implementation in the
same message, answer the question briefly first, then follow the normal task
classification and routing gates before making changes.

---

## Task Classification

Before any file is created, edited, or deleted, state the task classification.
For every non-trivial task, as determined by
`.claude/skills/task-routing/SKILL.md`, use that skill and do not implement until
it emits `Manager: manager - output below`. TaskPilot-only administration is
exempt from routing; `.claude/skills/taskpilot-work/SKILL.md` owns that
procedure. This exemption applies only to metadata-only administration; the
underlying non-trivial work still requires its TaskPilot identity and branch
decision.

---

## Task Identity And Tracking

TaskPilot (project key **WP**) is the source of truth for non-trivial product
work and product/documentation maintenance. An existing TaskPilot item is
required before implementation for that work.

**AI-governance maintenance is TaskPilot-exempt.** This exemption covers this
root `AGENTS.md` contract and the operational materials under `.claude/`
(skills, agents, pipelines, conventions, SDD tooling, and templates) when the
change governs AI execution rather than product behavior or product
documentation. It does not exempt `docs/`, feature specifications, or any
product/runtime artifact. Exempt work still requires classification, the
appropriate routed quality gates, the Git-operation decision, and final
closure evidence. **TaskPilot-exempt is not the Quality Tier `Exempt`:**
non-trivial AI-governance work remains Full tier.

`.claude/skills/taskpilot-work/SKILL.md` owns item lookup, approval, record
structure, commands, and lifecycle procedure for tracked work.

### Git Operation Authority

Do not create, switch to, publish, delete, merge, or otherwise mutate a Git
branch without the user's explicit approval. Approval to create or update a
TaskPilot item is not approval for any Git operation. Without branch approval,
trivial work remains on the current branch. For non-trivial work, if the user has
not explicitly approved either staying on the current branch or using a new
branch, stop and ask before editing.

---

## Quality Tiers

- **Full** — all non-trivial product, engineering, instruction-system, high-risk, or system-level work. Apply every routed quality gate.
- **Lite** — low/medium-risk non-trivial reference documentation or visual-only work with no runtime behavior change. Require a defined target and Definition of Done, the routed git gate, relevant validation, and task-complete.
- **Exempt** — trivial work only. Trivial fixes and changes need no TaskPilot item.
- When work qualifies for more than one tier, use the higher tier.

---

## Quality Gates

These apply to all non-trivial work and may not be skipped:

- Non-trivial logic must satisfy the TDD provenance gate in
  `.claude/skills/testing-pro/SKILL.md` before production edits.
- UI changes require the manual verification selected by
  `.claude/skills/validate/SKILL.md`.
- Documentation maintenance, local validation, and DoD closure must use their
  routed owning skills.

## Commit And Push Boundary

Never push changes unless the user explicitly requests it — this applies
unconditionally, including during pipeline execution, code review cycles, and
validation runs.

For tracked implementation work, a local task-scoped commit is required before
the TaskPilot item may transition to `done`. The user's approval to implement a
tracked task authorizes that local completion commit; it does not authorize a
push. A normal delivery uses one commit per task. The sole exception is an
explicitly declared two-task delivery cohort, whose one shared commit must name
both TaskPilot IDs and is evidence for both items. AI-governance maintenance
remains subject to the normal explicit commit request unless the user directs
otherwise. Push still requires an explicit prompt every time regardless. See
`.claude/skills/work-with-git/SKILL.md` for implementation details.

---

## Final Response Gate

For non-trivial work, use `.claude/skills/task-complete/SKILL.md` to report the
selected route's required evidence. A failed check, missing requirement, or
unsafe/unimplemented behavior blocks completion. An external post-implementation
check that is explicitly documented as a verification limitation is not missing
evidence.

---

## Instruction System

The instruction system is a curated engineering core plus the Spec-Driven
Development (SDD) tooling. Its capabilities live under `.claude/skills/`,
`.claude/agents/`, `.claude/pipelines/`, `.claude/conventions/`, and
`.claude/sdd/` (doc templates); `.claude/skills/task-routing/SKILL.md` is the
authoritative router and lists every available route. Material
instruction-system changes are themselves non-trivial and route through
`task-routing`.

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
