# AGENTS.md — WhisperPilot Root Contract

All AI tools must read this file before working in WhisperPilot. It defines
project-wide policy, exceptions, authoritative sources, and a portable
capability-discovery index. Referenced skills, agents, pipelines, and
conventions are the sole authorities for their procedures.

## Agent And Subagent Execution

An applicable project instruction, pipeline, or routed handoff that names an
agent or subagent is standing user authorization to invoke it. A routed agent
handoff must use a distinct agent and must not run inline. Preserve the invoked
agent's declared state-mutation boundary; `agent-handoff` owns runner
selection, eligibility, and recovery.

## Project

WhisperPilot is a macOS application for offline transcription of local audio
and video, with speaker attribution and local-LLM summaries. It has no live
capture or cloud service. `docs/idea.md` owns product scope; `docs/architecture.md`
owns technical architecture.

## Question Before Action

Answer questions, assessments, and reviews before implementation. Read-only
inspection is permitted to answer accurately; mutate files or state only when
the user explicitly asks to proceed. A mixed question and implementation
request follows this order, then the applicable gates below.

## Task Classification

Before creating, editing, or deleting a file, state `<complexity>; <risk>;
<domain>`. Use the vocabulary and boundary in
`.claude/skills/task-routing/SKILL.md` §Classification.

- For trivial work, execute directly and load a matching capability from the
  discovery index. No TaskPilot item is required.
- For non-trivial or uncertain work, load `task-routing` and do not implement
  before its `Manager: manager - output below` artifact.
- TaskPilot-only administration is exempt from routing; `taskpilot-work` owns
  it. Underlying work remains subject to its normal identity and Git gates.

## Non-Negotiable Gates And User Waivers

Classification, non-trivial routing, non-trivial Git branch decisions, and
system-level approval are mandatory stop gates. A waiver applies only to the
named obligation; broad wording such as “just fix it” does not waive a gate.

### Explicit No-TaskPilot Override

An explicit instruction not to create or use TaskPilot items is binding for
that request. It waives the work-process gates that exist to prepare, require,
validate, or close TaskPilot-tracked work, including routing, lifecycle,
branch-decision, quality, review, commit, and closure artifacts. It does not
authorize destructive Git operations, external writes, or actions beyond the
user's stated scope.

## Task Identity And Tracking

TaskPilot (project key **WP**) is the source of truth for non-trivial product
and product-documentation work; an existing item is required before
implementation. `taskpilot-work` owns record lookup, approval, fields, and
lifecycle.

When the user explicitly designates existing paths as user-authored worktree
changes for review, validation, remediation, commit, or push, use identity
`untracked — user-authored worktree review` and do not create or reopen a
TaskPilot item solely for that closure work. The direct review route in
`task-routing` owns its frozen boundary, coverage, remediation, validation,
review, and commit requirements. Scope expansion or new behavior returns to
normal tracked work.

When the user supplies review findings (including GitHub or CI findings) for a
named existing task branch or pull request, treat an accepted finding as
**post-review remediation** of that task. Keep the correction on that named
branch and use the existing task ID for commits. Do not create, reopen, or
mutate a TaskPilot item solely for the finding, even when the original item is
already `done`. The direct post-review route in `task-routing` owns its
focused test, remediation, validation, review, Git, and closure evidence.
This exception is limited to correcting the reported finding; a new behavior,
an unrelated path, or a user request for separately tracked work returns to
normal TaskPilot routing.

AI-governance maintenance of `AGENTS.md` or `.claude/` is TaskPilot-exempt
when it governs AI execution only; it never exempts `docs/` or
product/runtime artifacts. Exempt non-trivial work remains Full tier and uses
its routed quality, Git, and closure gates.

### Git Operation Authority

Do not create, switch, publish, delete, merge, or otherwise mutate a branch
without explicit user approval. Never discard, overwrite, or rewrite
uncommitted user changes without explicit approval. Approval to create a task
branch authorizes exactly one immediate `git push -u origin <branch>` after
creation; every other push needs an explicit request in the current
instruction. `work-with-git` owns branch selection, commit composition, and
recovery.

Tracked task code and related TaskPilot records form one atomic task-scoped
local commit. AI-governance maintenance commits require an explicit user
request.

## Quality Tiers

| Tier | Applies to | Minimum policy |
|---|---|---|
| Full | Non-trivial product, engineering, instruction-system, high-risk, or system-level work | Apply every routed quality gate. |
| Lite | Low/medium-risk reference documentation or visual-only work with no runtime behavior change | Defined target, Lite readiness confirmation, DoD, routed Git gate, relevant validation, and closure. |
| Exempt | Trivial work only | No TaskPilot item. |

Use the higher tier when more than one applies.

## Quality And Closure Gates

Every non-trivial route applies its required test, validation, documentation,
review, and DoD gates. Non-trivial logic needs `testing-pro` TDD provenance
before production edits; UI work uses the manual verification selected by
`validate`. `documentation-maintenance`, `validate`, `task-quality`, and
`task-complete` own their respective procedures. Do not report completion with
failed required evidence or unsafe/unimplemented behavior.

Significant transcription changes require the real-Metal transcription gate
and its own validation-report row. `test-runner` owns the impact boundary,
execution, environment handling, reporting procedure, and CI exclusion.

## Final Response Gate

For non-trivial work, use `task-complete` to report the selected route's
required evidence, including any documented external-verification limitation.

## Instruction System

This discovery index keeps the project usable by AI tools without
platform-specific skill discovery. It is an index only: the referenced file is
the procedural authority. `task-routing` is the canonical route-selection map.
Unless an entry states otherwise, a skill resolves to
`.claude/skills/<name>/SKILL.md` and an agent resolves to
`.claude/agents/<name>.md`.

### Skills

| Capability | Trigger |
|---|---|
| `agent-handoff` | Run a routed distinct-agent handoff. |
| `brainstorm` | Discuss an open design decision with trade-offs. |
| `design-in-pen` | Create or iterate on a pre-implementation `pencil/*.pen` mockup. |
| `discover-requirements` | Elicit unclear feature, epic, or task requirements. |
| `documentation-maintenance` | Check documentation after a feature, refactor, or non-trivial fix. |
| `implement-tauri-feature` | Implement routed feature or confirmed-fix work. |
| `react-tauri-expert` | Give read-only React/TypeScript/Tauri advice. |
| `record-discovered-spec` | Persist an approved discovery specification to TaskPilot. |
| `sdd-doc-author` | Create or update one SDD document. |
| `sdd-index-sync` | Rebuild `docs/INDEX.md` after an SDD change. |
| `sync-pen-code` | Synchronize `pencil/*.pen` and UI code outside implementation. |
| `task-complete` | Close non-trivial routed work. |
| `task-quality` | Map completion evidence to the DoD. |
| `task-routing` | Classify and route non-trivial work. |
| `taskpilot-work` | Manage TaskPilot records. |
| `testing-pro` | Write or improve Vitest or Rust tests. |
| `triage-bug` | Find an unknown cause of a reported bug. |
| `validate` | Run local CI-equivalent checks. |
| `verify-readiness` | Check the Definition of Ready before implementation. |
| `work-with-git` | Apply a routed branch, commit, or push boundary. |

### Agents

| Agent | Trigger |
|---|---|
| `code-reviewer` | Review a completed, validated implementation diff. |
| `instruction-evaluator` | Review one changed AI-governance artifact. |
| `pencil-vision-reviewer` | Check a rendered `pencil/*.pen` mutation. |
| `scope-verifier` | Check structural completeness of a draft requirements specification. |
| `sdd-gap-analyzer` | Inventory docs-versus-code SDD gaps. |
| `sdd-spec-reviewer` | Review SDD completeness and traceability. |
| `test-runner` | Execute and report routed validation. |

## Version Management

Before every commit, run `scripts/bump-version.sh` before staging: `minor` for
fixes/refactors/chores (`1.9.0` → `1.9.1`), `major` for user-facing features
(`1.9.1` → `1.10.0`), and `release` only
when explicitly requested. Use the highest applicable bump for the pending
commit; the script updates both Tauri version files.

## Spec-Driven Development

`.claude/conventions/sdd-doc-set.md` owns the Standard SDD document set, its
structure, and ADR identifiers. Feature, task, and scenario identifiers and
their traceability belong to TaskPilot (see §Task Identity And Tracking);
`docs/` neither assigns nor mirrors them.

## Authoritative Sources

| Source | Purpose |
|---|---|
| `docs/INDEX.md` | Documentation and decision-log map. |
| `docs/idea.md` | Product scope and principles. |
| `docs/architecture.md` | Technical architecture. |
| `docs/design.md` | UX flows, screens, and states. |
| `docs/testing.md` | Test strategy and quality gates. |
| `docs/roadmap.md` | Milestones and sequencing. |
| `docs/decisions/` | Architecture decision records. |
| `README.md` | User-facing overview and setup. |
| `docs/development.md` | Developer setup and commands. |
| `src-tauri/src/lib.rs`, `src-tauri/src/main.rs`, `src/App.tsx` | Application entry points. |
