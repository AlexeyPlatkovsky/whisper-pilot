# AGENTS.md — WhisperPilot Root Contract

This file is the root operational contract for the WhisperPilot project.
All AI tools working on this project must read this file before starting any work.
This file overrides any tool-specific adapter on conflict.

## Agent And Subagent Execution

Distinct-agent handoffs must preserve the invoked agent's declared state-mutation
boundary and must not be executed inline. A runner with an exact declared tool
and sandbox boundary is preferred; a broader runner may be used only when it is
explicitly constrained to the invoked agent's declared boundary and the handoff
records that exception. When runner selection or recovery is necessary, use
`.claude/skills/agent-handoff/SKILL.md`. An agent already running in an isolated
execution context executes its own instructions directly.

An applicable project instruction, pipeline, or routed handoff that explicitly
names an agent or subagent is standing user authorization to invoke that agent.
Do not request separate user approval for that invocation.

---

## Project

WhisperPilot is a macOS desktop application for **offline transcription of local
audio and video files**, with speaker attribution and a local-LLM summary. 
There is no live capture and no cloud. Product scope, milestones, and engines 
are owned by `docs/idea.md`; technical architecture is owned by `docs/architecture.md`. 
Do not restate them here.

---

## Question Before Action

When the user asks a question, requests an assessment or review, or asks whether
something is possible, answer the question before taking implementation action.
Do not interpret questions such as requests to make changes.

Read-only inspection is permitted when needed to answer accurately. Do not
create, edit, or delete files; run state-changing commands; or initiate work
unless the user explicitly asks to implement, modify, run, execute, or otherwise
proceed with the action.

If the user asks both a question and explicitly requests implementation in the
same message, answer the question briefly first, then follow the normal task
classification and routing gates before making changes.

---

## Task Classification

Stating the task classification is a required output. Before any file is
created, edited, or deleted, state it explicitly in your response as
`<complexity>; <risk>; <domain>`. If that line is absent, the classification
gate has not been satisfied and no file may be touched. This line is the minimum
gate; non-trivial work additionally states the Quality Tier and full
classification through `.claude/skills/task-routing/SKILL.md`.
For every non-trivial task, as determined by
`.claude/skills/task-routing/SKILL.md`, use that skill and do not implement until
it emits `Manager: manager - output below`. TaskPilot-only administration is
exempt from routing; `.claude/skills/taskpilot-work/SKILL.md` owns that
procedure. This exemption applies only to metadata-only administration; the
underlying non-trivial work still requires its TaskPilot identity and branch
decision.

---

## Non-Negotiable Gates And User Waivers

The classification gate, the `task-routing` gate for non-trivial work (subject
only to the routing exemptions defined in §Task Classification), the Git branch
decision gate for non-trivial work, and the system-level approval stop are
mandatory stop gates. They may not be skipped, deferred, or collapsed for
convenience. For trivial work the branch gate resolves by defaulting to the
current branch per §Git Operation Authority.

A user waiver is interpreted literally and minimally: it removes only the
specific obligation the user named, and nothing adjacent. Waiving a TaskPilot
item — or any single artifact — never waives classification, routing, the routed
quality gates, or the branch decision. A terse instruction ("just fix it", "no
ticket", "quick change") is not a waiver of any gate. When a broad instruction
could be read as skipping a gate, treat it as authorization to do the work, not
permission to skip the gate, and proceed through the gate. To skip a mandatory
gate the user must say so explicitly for that specific gate.

An operating mode that biases toward acting without pausing — for example, Auto
Mode — changes only whether to ask optional clarifying questions. It never
overrides a mandatory stop gate and never converts an ambiguous instruction into
a waiver. When bias-to-action and a stop gate conflict, the stop gate wins.

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

Never push changes unless the user explicitly requests a push in the current
instruction. This applies unconditionally, including after branch creation,
during pipeline execution, code review cycles, validation runs, and task
completion. Approval to create a branch, commit a task, or mark an item done
never authorizes a push.

For tracked implementation work, the task-scoped local commit must include the
related TaskPilot records: item specification changes, lifecycle state, and
TaskPilot comments created during the task. Prepare the completion comment and
perform the verified `in_progress → done` transition before staging; then stage
the code and every related `.taskpilot/` change and create the one local task
commit. This keeps the committed code and its TaskPilot evidence atomic. Report
the resulting commit hash in the closure record; do not write it back to
TaskPilot afterward, because that would leave lifecycle metadata uncommitted.
A normal delivery uses one commit per task. The sole exception is an explicitly
declared two-task delivery cohort, whose one shared commit must name both
TaskPilot IDs and include both records. AI-governance maintenance remains
subject to the normal explicit commit request unless the user directs otherwise.
See `.claude/skills/work-with-git/SKILL.md` for implementation details.

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
