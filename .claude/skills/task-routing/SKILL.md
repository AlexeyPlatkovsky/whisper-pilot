---
name: task-routing
description: Classifies and routes non-trivial work to the correct pipeline or capability, then declares the route's required handoffs.
---

## Purpose

Classify non-trivial tasks, select the correct execution route, and declare its required handoffs. The selected pipeline owns step order and gate conditions.

The manager routes. It does not execute steps.

## When This File Is Loaded

Load when `AGENTS.md` classification gate fires for non-trivial work.

If a task turns out to be trivial after review, say so and release it for direct execution.

When `AGENTS.md` classifies work as trivial, release it for direct execution.
This skill does not participate in trivial work, including its architecture and
git gates.

## Classification

Before selecting any pipeline or capability, classify out loud:

| Dimension | Options |
|---|---|
| Complexity | trivial / non-trivial |
| Risk | low / medium / high / system-level |
| Domain | Tauri/React/Rust feature / UI design variant / CLI adapter / product documentation / AI-governance instruction system / bug triage / bug fix / other |
| Quality Tier | Project quality-tier decision |

When unsure of complexity: treat as non-trivial.
When unsure of risk: treat as medium.
Apply `AGENTS.md` §Quality Tiers and state the selected tier, its justification, and any tier-specific readiness confirmation required there.

Classification must be stated before any file is created, edited, or deleted.

### Phase-Level Requests

A request to implement a named phase is not a single task. Use
`.claude/skills/taskpilot-work/SKILL.md` to identify items tagged with the named
phase, then route each unfinished child independently. Do not create a phase
wrapper item. If no child is registered, ask the user how to proceed; if all are
done, report that result without re-routing them.

## Task Identity, Lifecycle, And Git Branch Gate

Apply `AGENTS.md` §Task Identity And Tracking before routing any branch decision.

For tracked work, inspect the existing TaskPilot item and its parent context
before routing. The manager must declare the item status and the lifecycle
operation that will be required before implementation starts. A tracked item is
not eligible for implementation from `backlog`; it must be `ready`, or be an
explicitly resumed `blocked` item. For an already-approved legacy/backlog item,
first run DoR and route the verified `backlog → ready` promotion through
`taskpilot-work`; otherwise route `discover-feature`.

For AI-governance instruction-system work exempted by `AGENTS.md`, declare
`TaskPilot: exempt — AI-governance maintenance` and do not create or mutate a
TaskPilot item unless the user explicitly requests TaskPilot administration.

For every non-trivial task, route `.claude/skills/work-with-git/SKILL.md` after
the task-identity/lifecycle decision and before implementation or artifact
changes begin, subject to the branch-approval rule in `AGENTS.md` §Git Operation
Authority.

The manager only routes the git branch gate. The skill decides whether to create a branch, stay on the current branch, or ask the user when branch ownership is ambiguous.

## Architecture Documentation Gate

For non-trivial product or engineering work that touches or depends on existing UI, IPC, Rust core, the audio-ingest/transcription/diarization/summarization pipeline, external tools (ffmpeg), models, or file I/O, require focused architecture loading:

1. Read `docs/architecture.md`.
2. Focus on only the sections matching the task: Layer Map, Audio Ingestion, Transcription, Diarization, Summarization, IPC Contract, or Security And Privacy.

Skip this gate only for named low-value categories: narrow instruction-only changes, pure copy edits, documentation-only restructuring that does not change architecture facts, or isolated command execution.

When routing implementation, discovery, triage, or review work, include the architecture-doc decision in the visible manager output: required with focused file(s), or skipped with reason.

## Routing

Apply this precedence before using the table:

1. Requirement discovery is disqualified when a complete approved specification
   exists and no re-scoping is requested, or when the user is describing a defect,
   crash, or unexpected behavior.
2. Behavior or IPC work takes precedence over visual-only UI work.

| Task | Route |
|---|---|
| Discover, specify, scope, or refine feature requirements | `.claude/pipelines/discover-feature.md` |
| Implement behavior, IPC, UI, or Rust core | `.claude/pipelines/implement-feature.md` |
| Triage a bug or unexpected behavior with unknown root cause | `.claude/skills/triage-bug/SKILL.md` |
| Fix a confirmed bug with reproduction steps | `.claude/pipelines/fix-bug.md` |
| Review or improve React/TypeScript/Tauri code practices | `.claude/skills/react-tauri-expert/SKILL.md` |
| Write or improve Vitest or Rust test code | `.claude/skills/testing-pro/SKILL.md` |
| Review test code or test changes | `.claude/agents/code-reviewer.md` |
| Validate completed work with build, test, or manual checks | `.claude/agents/test-runner.md` |
| Resolve an open design decision with meaningful trade-offs | `.claude/skills/brainstorm/SKILL.md` |
| Create or update non-instruction reference documentation | Direct execution with the required TaskPilot identity, git gate, `.claude/skills/documentation-maintenance/SKILL.md` outcome artifact (including the checked authoritative sources), and task-complete closure |
| Author or update one SDD main or extension document (`docs/idea.md`, `architecture.md`, `design.md`, `testing.md`, `roadmap.md`, an ADR, or an extension doc) | `.claude/skills/sdd-doc-author/SKILL.md` |
| Scaffold or update one SDD feature folder | `.claude/skills/sdd-feature-author/SKILL.md` |
| Rebuild `docs/INDEX.md` after an SDD change | `.claude/skills/sdd-index-sync/SKILL.md` |
| Adopt or expand an SDD docs tree | `.claude/pipelines/sdd-adopt.md` |
| Bootstrap an SDD docs tree from an empty root | `.claude/pipelines/sdd-bootstrap.md` |
| Assess gaps before introducing or expanding SDD | `.claude/agents/sdd-gap-analyzer.md` |
| Review an SDD docs tree for completeness and traceability | `.claude/agents/sdd-spec-reviewer.md` |

The SDD-document route applies only when `docs/INDEX.md` exists and lists the document, or the user explicitly frames the request as SDD / spec-driven-development work.

If the task does not match any route:

- stop
- classify and describe the task out loud
- ask the user to clarify or choose the correct path

## Route Declaration

The manager declares whether documentation maintenance and task-complete are required; the selected pipeline owns when those handoffs run. Apply `AGENTS.md` §Final Response Gate when reporting the route's closure requirements.

For an ad-hoc task with no matching pipeline, declare the direct capability and every required handoff in the output table. If the task has more than three discrete steps or crosses multiple files, add a non-gating recommendation for a dedicated pipeline after closure.

## Risk Escalation

| Risk | Requirement |
|---|---|
| Low / medium | Pipeline + local validation via `Skill: validate` for touched layers |
| High | Pipeline + `.claude/agents/code-reviewer.md` review or manual code review before closing |
| System-level | Stop and require explicit user approval before any file changes |

## Output Contract

At routing time, emit:

`Manager: manager - output below`

Use this table with no omitted rows:

| Field | Decision | Evidence / required artifact |
|---|---|---|
| Classification | `<complexity>; <risk>; <domain>; <quality tier>` | tier justification and readiness confirmation |
| Task identity | `<TaskPilot ID and status>` / `exempt — AI-governance maintenance` | `AGENTS.md` task-identity decision |
| Lifecycle | `required` / `not applicable` | tracked work: required start, block/resume, completion, and reload-verification artifacts; exempt work: reason |
| Git gate | `required` / `completed` / `blocked` | `Skill: work-with-git - output below` when complete |
| Route | `<pipeline or capability path>` | selected route |
| Validation | `<required checks and reviewers>` | expected validation artifacts |
| Architecture context | `required` / `skipped` | focused files or skip reason |
| Documentation maintenance | `required` / `not required` | trigger reason and expected artifact when required |
| Closure | `required` | `Skill: task-complete - output below` |
