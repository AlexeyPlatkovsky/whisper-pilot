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

`trivial` is limited to a single-file, non-behavioral correction with no
runtime, authority, lifecycle, security, persistence, dependency, generated
artifact, or cross-reference effect. Everything else is `non-trivial`.

**Exception — exploratory generated-artifact mutation.** A mutation that
produces or updates a generated artifact stays trivial-eligible (its other
disqualifiers — runtime, authority, lifecycle, security, persistence,
dependency, cross-reference — still apply and still disqualify it if any are
present) only when *all* of the following hold:
1. The artifact — evaluated at the smallest independently-mutable unit its
   producing tool addresses (e.g. one frame/component in a design file, not
   necessarily the whole file) — is a design/planning medium, not
   runtime/production code, a capability/config file, or a documented
   authoritative fact. "Documented authoritative fact" means either listed in
   `AGENTS.md`'s authoritative-source table, or covered by an
   authoritative-root rule a specific skill declares for its own domain (for
   example, `.claude/skills/documentation-maintenance/SKILL.md` §2 declares
   `pencil/*.pen` content that mirrors shipped UI as authoritative even
   though `AGENTS.md`'s table doesn't separately list it) — check both, not
   only the root table.
2. The mutation is reversible/re-runnable via the same tool that produced it,
   with no manual recovery step.
3. Nothing yet consumes or depends on the artifact's current state — no
   downstream skill or pipeline has been handed the result.

The moment any of these three stops holding for a given artifact unit — most
commonly, once its content is approved and handed to a consuming step —
classify normally as `non-trivial` from that point forward; a prior trivial
run of the same skill does not retroactively cover the now-non-trivial one.
This exception exists for genuinely exploratory work (e.g.
`.claude/skills/design-in-pen/SKILL.md`); it does not apply to a mutation any
other skill, pipeline, or shipped feature already reads.

Risk is selected by the highest matching condition:

| Risk | Observable condition |
|---|---|
| low | Read-only work, or a reversible non-behavioral edit within one owning artifact |
| medium | Multi-file or behavioral work without user-data, security, permission, persistence, or external-effect impact |
| high | User data, persistence, permissions/security, external process/network behavior, or difficult rollback |
| system-level | Root/routing/lifecycle/safety authority changes or repository-wide execution-policy changes |

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

When `AGENTS.md`'s User-authored worktree review exception applies, declare
`TaskPilot: untracked — user-authored worktree review`, identify the exact
pre-existing diff boundary as the pre-review `HEAD` SHA, exhaustive path list,
captured `git diff <SHA> -- <paths>` snapshot, and for every untracked regular
UTF-8 text path its status plus `git diff --no-index /dev/null <path>` snapshot;
block binary, non-regular, or undecodable untracked paths. Set Parent context
and Lifecycle to `not applicable`. Do not create, mutate, or require TaskPilot records. The
selected route's quality, validation, review, Git, documentation, and closure
gates remain required; only TaskPilot identity, lifecycle, and TaskPilot-record
commit requirements are waived.

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
3. A request to implement or build an approved Pencil design into working
   code always routes to `.claude/pipelines/implement-feature.md`, never to
   the standalone `sync-pen-code` row below — `sync-pen-code`'s `pen-to-code`
   direction never writes app code by itself; it only runs as that
   pipeline's internal Step 1a. The standalone `sync-pen-code` row applies
   only when no code implementation is requested this turn.

| Task | Route |
|---|---|
| Discover, specify, scope, or refine feature requirements | `.claude/pipelines/discover-feature.md` |
| Implement behavior, IPC, UI, or Rust core | `.claude/pipelines/implement-feature.md` |
| Triage a bug or unexpected behavior with unknown root cause | `.claude/skills/triage-bug/SKILL.md` |
| Fix a confirmed bug with reproduction steps | `.claude/pipelines/fix-bug.md` |
| Review or advise on React/TypeScript/Tauri code practices | `.claude/skills/react-tauri-expert/SKILL.md` |
| Review, validate, remediate, commit, or push a user-designated, pre-existing working-tree diff | Manager-declared direct user-authored worktree review route: frozen boundary record; review; conditional direct remediation; validation; code review; documentation decision; local commit/push; closure |
| Implement a requested React/TypeScript/Tauri improvement | `.claude/pipelines/implement-feature.md` |
| Write or improve Vitest or Rust test code | `.claude/skills/testing-pro/SKILL.md` |
| Review completed, validated test changes | `.claude/agents/code-reviewer.md` |
| Review existing test code without a change diff | Manager-declared read-only test audit with explicit scope; no post-implementation validation prerequisite |
| Validate completed work with build, test, or manual checks | `.claude/agents/test-runner.md` |
| Resolve an open design decision with meaningful trade-offs | `.claude/skills/brainstorm/SKILL.md` |
| Create or iterate on a UI design mockup in `pencil/*.pen` before implementation | `.claude/skills/design-in-pen/SKILL.md` |
| Sync `pencil/*.pen` with the real UI code in either direction, outside of `implement-feature`'s or `documentation-maintenance`'s automatic hook (e.g. a bare design→code translation with no implementation this turn, or a post-hoc drift correction) — subject to precedence rule 3 above | `.claude/skills/sync-pen-code/SKILL.md` |
| Create or update non-instruction reference documentation | Direct execution with the required TaskPilot identity, git gate, `.claude/skills/documentation-maintenance/SKILL.md` outcome artifact (including the checked authoritative sources), and task-complete closure |
| Author or update one SDD main or extension document (`docs/idea.md`, `architecture.md`, `design.md`, `testing.md`, `roadmap.md`, an ADR, or an extension doc) | Direct sequence: `.claude/skills/sdd-doc-author/SKILL.md`, then `.claude/skills/sdd-index-sync/SKILL.md` with mode, same Route run, attempt `1` (prior artifact plus increment on retry), then closure |
| Create or rebuild `docs/INDEX.md` after an SDD change | `.claude/skills/sdd-index-sync/SKILL.md` with mode, Route run, and sync attempt |
| Adopt or expand an SDD docs tree | `.claude/pipelines/sdd-adopt.md` |
| Bootstrap an SDD docs tree from an empty root | `.claude/pipelines/sdd-bootstrap.md` |
| Assess gaps before introducing or expanding SDD | `.claude/agents/sdd-gap-analyzer.md` |
| Review an SDD docs tree for completeness and traceability | `.claude/agents/sdd-spec-reviewer.md` |
| Review one AI-governance instruction artifact | `.claude/agents/instruction-evaluator.md` through `.claude/skills/agent-handoff/SKILL.md` |
| Change AI-governance instruction artifacts | Manager-declared ad-hoc direct maintenance route with Git gate, one isolated `instruction-evaluator` review per changed artifact, structural integration validation, and task-complete closure |
| Review or change a CLI adapter | Manager-declared ad-hoc CLI route; require the adapter's invocation-contract evidence, tests through `testing-pro`, implementation through `implement-feature` when behavior changes, and code review |

The SDD-document route applies only when `docs/INDEX.md` exists and lists the document, or the user explicitly frames the request as SDD / spec-driven-development work.

### User-Authored Worktree Review Direct Route

Use only for the routing-table row above. Maintain a Route execution record
with these planned rows: `B1` frozen-boundary record; `B2` manager initial
diff-review record plus, when frozen logic is present, the labeled existing-
coverage assessment; `B3` conditional `Skill: testing-pro - output below` Red
evidence; `B4` remediation or its exact skip condition; `B5` `Agent:
test-runner - output below` final validation with manager-declared commands;
`B6` `Agent: code-reviewer - output below`; `B7` `Skill:
documentation-maintenance - output below` when its observable trigger applies,
otherwise a manager documentation-decision record with the exact skip
condition; `B8` conditional local commit and authorized push; `B9` closure.
The manager owns objective DoD criteria for the
frozen boundary. A remediation is eligible only when B2, B5, or B6 demonstrates
a defect, regression, build failure, or validation failure within that boundary;
otherwise B4 is skipped. B3 is required before a remediation that changes
non-trivial production logic and skipped otherwise. A finding in B5 or B6
returns to B3/B4, invalidates the prior B5/B6 attempts, and requires fresh
validation and code review before B8. B8 runs when the user explicitly
requested a commit or push; commit and push each run only when explicitly
requested, otherwise record the exact skip condition. A push-only request
requires an already-existing eligible local commit; otherwise block and request
commit authorization. Any new behavior,
unrelated path, or changed baseline invalidates this route and returns the work
to normal TaskPilot routing.

For frozen user-authored logic, the B2 assessment maps every changed observable
behavior to a focused passing test or explicitly marks it missing.

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
| Low / medium mutation | Selected mutation route + artifact-appropriate validation |
| Low / medium read-only | Stable review/assessment artifact; no build validation |
| High | Selected mutation route + `.claude/agents/code-reviewer.md` or the domain-specific reviewer before closing |
| System-level | Stop and require explicit user approval before any file changes |

## Output Contract

At routing time, emit:

`Manager: manager - output below`

Use this table with no omitted rows:

| Field | Decision | Evidence / required artifact |
|---|---|---|
| Route run | `<task identity>:<route>:<positive sequence>` | stable identifier unique to this routed invocation; use `untracked-user-authored-worktree-review:<route>:<positive sequence>` for untracked work and bind it to the frozen boundary |
| Classification | `<complexity>; <risk>; <domain>; <quality tier>` | tier justification and readiness confirmation |
| Task identity | `<TaskPilot ID and status>` / `exempt — AI-governance maintenance` / `untracked — user-authored worktree review` | `AGENTS.md` task-identity decision |
| Pre-existing diff boundary | `required` / `not applicable` | untracked: pre-review `HEAD` SHA, exhaustive paths, SHA-based tracked-path snapshot, and no-index snapshot for each regular UTF-8 text untracked path; otherwise reason |
| Parent context | `<expected parent ID>` / `none` / `not applicable` | tracked work: reloaded item parent; exempt/untracked work: reason |
| Definition of Done | stable `C-<positive integer>` criteria | objective pass/fail criteria; required explicitly for AI-governance work |
| Execution record | `required` | pipeline-owned record, or complete ad-hoc step plan using the task-complete binding schema |
| Lifecycle | `required` / `not applicable` | tracked work: required start, block/resume, completion, and reload-verification artifacts; exempt/untracked work: reason |
| Git gate | `required` / `completed` / `blocked` | `Skill: work-with-git - output below` when complete |
| Route | `<pipeline or capability path>` | selected route |
| Validation | `<required checks and reviewers>` | expected validation artifacts |
| Architecture context | `required` / `skipped` | focused files or skip reason |
| Documentation maintenance | `required` / `not required` | trigger reason and expected artifact when required |
| Closure | `required` | `Skill: task-complete - output below` |
