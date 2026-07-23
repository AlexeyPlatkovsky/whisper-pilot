---
name: discover-requirements
description: Structured Q&A to elicit complete, unambiguous requirements for a feature, epic, or task before implementation begins. Never guesses at unclear points — always asks.
---

# Skill: discover-requirements

## Purpose

Surface every requirement owned at the routed item altitude. Lower-altitude
details must be assigned explicitly to children rather than guessed or forced
prematurely.

## When This Skill Applies

Use only when `.claude/pipelines/discover-feature.md` invokes mode `initial`,
`gap-re-entry`, or `approval-revision`. Direct invocations outside that
pipeline return to `.claude/skills/task-routing/SKILL.md`.

## Context Loading

Before asking any questions, require the manager artifact's exact Route run,
the routed TaskPilot item type (`epic`/`feature`/`task`), and:

1. Read the relevant feature requirements, scenarios, and task records under
   `docs/features/`, when they exist.
2. If the requested work touches or depends on existing UI, IPC, Rust core, or database behavior, read the relevant sections of `docs/architecture.md`.
3. State which architecture docs were checked, or "Architecture docs skipped: <reason>."

## Q&A Rounds

Carry forward source-cited facts already explicit in the initial request,
existing records, and loaded authorities. Ask only unanswered or conflicting
questions, one round at a time.

If this is a **gap-targeted re-entry** after scope-verifier found gaps, use the
current draft, gap table, and new user answers to revise only affected rounds.
If this is an **approval revision** after a verified draft was rejected or
changed, use the prior verified draft, `No gaps` artifact, and user changes to
revise affected rounds. In both modes, increment the draft revision, recompute
the digest, preserve scenario IDs by the rules below, and do not restart from
Round 1.

### Altitude Calibration (progressive elaboration)

Apply the rounds at the depth the item's **altitude** warrants. Detail that belongs to a lower altitude is deferred to that level, not forced prematurely — eliciting task-level edge cases while scoping an epic produces noise, and skipping them while defining a task produces rework.

- **Epic — high level.** Emphasize Round 1 (goal & user) and Round 6 (scope & child breakdown). Capture the user-visible outcome, the scope boundary, and the expected feature/child breakdown. Answer Rounds 2–5 only at a *coordinating* level (which child features carry the happy path, which own the principal failure modes) — do **not** drill into concrete scenarios, decision tables, or interaction contracts. Those belong to the children.
- **Feature — medium level.** Emphasize Rounds 1–3 and 5: the user journey, the happy-path flow, the principal failure modes, and — for a UI surface — the **control elements and interaction contract**. In Round 4, capture the notable boundaries; defer exhaustive case enumeration to the child tasks. Identify the task breakdown.
- **Task — full depth.** All six rounds at concrete, testable depth. Rounds 3 (failures) and 4 (edge cases & boundaries) must be exhaustive enough to derive test cases by the techniques in `.claude/conventions/testing-taxonomy.md` §Test-Design Techniques, captured in the BDD scenarios. This is the altitude for implementation detail and adversarial cases.

A **standalone** item (no decomposition) is elicited at task altitude. Match the draft spec to the altitude: an epic's draft emphasizes child breakdown and omits concrete BDD scenarios (list expected child features instead); a feature's covers feature-level workflows; a task's carries the exhaustive scenarios.

### Round 1 — Goal & User

- What user problem or need does this address?
- Who is the user / what role or context triggers this?
- What does success look like from the user's perspective? (user-visible outcome, not mechanism)

### Round 2 — Happy Path

- What is the step-by-step flow for the normal case?
- What triggers it (user action, event, CLI output, timer)?
- What is the expected output or visible result?

### Round 3 — Failure & Error States

- What can go wrong at each step?
- How should each failure behave? (silent, visible error, fallback, retry)
- What happens when the underlying CLI / service is unavailable?

### Round 4 — Edge Cases & Boundaries

- What is the empty/zero state? (no items, no data, first launch)
- What are boundary inputs? (very long text, special characters, maximum count)
- Does concurrent access matter? (two prompts in flight, rapid re-trigger)
- Any macOS-version or WKWebView-specific behavior differences?

### Round 5 — Constraints

- Performance or latency expectations? (or explicit "none")
- Security or permission requirements? (or explicit "none")
- If a UI surface: what is the interaction contract? (drag, keyboard, sizing, default state the user starts from)

### Round 6 — Scope & Integration

- What is explicitly OUT of scope for this work?
- Does this extend an existing epic, feature, or task?
- What existing work items are related or must remain unchanged?
- What files, modules, commands, interfaces, or other target surfaces are already known? If unknown, state that they must be identified during implementation.

## Rules

### Never Guess

If any answer introduces new ambiguity or an important question is unanswered, ask a follow-up in the same round before proceeding. Never proceed with "I'll assume X" — always ask.

### One Round at a Time

Present one round's questions, then stop and wait for the user's answers before presenting the next round.

### "None" Verification for Rounds 3 and 4

If the user provides only "none" or "not applicable" for all sub-questions in Round 3 (failures) or Round 4 (edge cases), do not accept that silently. Ask one verification follow-up before advancing:

> "Can you confirm there are genuinely no failure / edge-case scenarios for this feature? If so, please state that explicitly and I will note it as verified."

A confirmed explicit statement counts as answered. Silence or a vague "yeah" does not.

### Completeness Gate

Do not emit the draft spec until ALL six rounds have explicit answers. An answer counts as explicit only if it directly addresses the sub-question. A single word, a restated question title, or a vague qualifier ("it should work", "normal cases") does not qualify — re-ask that sub-question before advancing. An answer of "not applicable" or "none" is valid when it was explicitly confirmed (see "None Verification" above for Rounds 3 and 4).

If the user explicitly declines, cannot provide, or confirms that a required
answer is unknowable, stop rather than re-asking indefinitely. Emit the output
label followed by `Status: Blocked`, then a table with `Round`, `Unresolved
question`, `Reason`, and `Unblocking action`. Do not emit a draft. The invoking
pipeline applies its post-activation blocker rule.

At **epic** or **feature** altitude (see §Altitude Calibration), a
*coordinating-level* answer — one that names which child feature or task owns
the deferred detail — counts as an explicit answer for Rounds 2–5; the deferred
detail is not a gap at this altitude. A coordinating answer is required only
for the sub-questions that §Altitude Calibration permits to be deferred; answer
the remaining sub-questions at that altitude's required depth. Altitude lowers
the required depth, never the requirement to answer what that altitude owns.

## Draft Spec Format

When all rounds are complete, emit the draft spec using this structure exactly:

```
Draft version: <exact manager Route run>:<positive revision number>
Draft digest: sha256:<64 lowercase hexadecimal characters>

Type: [epic | feature | task]
Title: <concise imperative phrase>

Description:
<narrative written so an AI agent with no conversation context can identify: the user goal, the happy path, the primary failure mode, and the scope boundary; must address all four points explicitly>

Non-goals:
- <explicit out-of-scope item>

Acceptance criteria (DoD):
- [ ] <objectively verifiable condition>
- [ ] <objectively verifiable condition>

Constraints / design notes:
<performance, security, UI contract, platform notes — or "none">

Dependencies / integration:
<parent or related work, behavior that must remain unchanged, and external prerequisites — or "none">

Target surfaces:
<known files, modules, commands, or interfaces — or "to be identified during implementation">

Proposed BDD scenarios:
  Scenario S-1: <Happy path — one-line title>
    Given ...
    When ...
    Then ...

  Scenario S-2: <Error state — one-line title>
    Given ...
    When ...
    Then ...

  Scenario S-3: <Edge case — one-line title> (add as many as surfaced)
    Given ...
    When ...
    Then ...

Case-derivation evidence:
| Technique | Scenario IDs or justified N/A |
|---|---|
| Equivalence Partitioning | ... |
| Boundary Value Analysis | ... |
| Decision Table | ... |
| State-Transition | ... |
| Pairwise / Combinatorial | ... |

Child-task breakdown:
- <required for feature altitude; use "Not applicable — task altitude" for a task>

Child-feature breakdown:
- <required for epic altitude; use "Not applicable — feature/task altitude" otherwise>
```

Every draft requires at least two objectively verifiable DoD criteria; the two
rows shown above are illustrative placeholders, not a maximum. Start revision
at `1` and increment it whenever any canonical draft content changes. The
canonical digest payload is the exact UTF-8 draft text from `Type:` through the
final Child-feature breakdown row, with LF line endings, no trailing whitespace
on any line, and one terminal LF. Compute SHA-256 over that payload and emit it
as `sha256:<lowercase hex>`. A content change requires a new revision and digest.

Apply the fields by altitude:
- **Epic:** replace the entire Proposed BDD scenarios section and Child-task
  breakdown value with `Not applicable — epic altitude`; set case-derivation
  evidence to `Not applicable — lower altitude`; populate Child-feature
  breakdown. Keep Description, Non-goals, Constraints, and DoD at epic depth.
- **Feature:** replace the illustrative BDD scenario placeholders with
  feature-level scenarios, set case-derivation evidence to `Not applicable —
  child tasks own exhaustive derivation`, populate Child-task breakdown, and set
  Child-feature breakdown to `Not applicable — feature altitude`.
- **Task / standalone:** replace the illustrative BDD scenario placeholders
  with exhaustive scenarios; populate all five case-derivation rows with
  scenario IDs or a technique-specific justified N/A; set both child-breakdown
  fields to their task-altitude not-applicable values.

The three BDD blocks in the format are illustrative placeholders, not a
requirement to create exactly three scenarios. Include every scenario surfaced
at the applicable altitude. Allocate draft-local scenario IDs from `S-1`;
preserve an ID while its title and observable outcome remain the same across a
revision, remove resolved IDs without reuse, and assign new scenarios
`max(previous numeric ID)+1`.

For a UI-only task with no deeper logic, the required happy-path BDD scenario may
be the UI smoke path: it states the starting UI state, the functional interaction
or event, and the observable resulting state. It remains a Given/When/Then
scenario and must not be an imperative click script.

Dependencies / integration and Target surfaces remain required at every altitude, with detail calibrated to that altitude.

## Output Contract

When the draft spec is ready, begin the response with:

`Skill: discover-requirements - output below`

Then emit the draft spec in the format above.

Use `verified none` for a confirmed absence only where the altitude-specific
schema permits it; otherwise the absence is a gap. Do not emit until every
required field for the routed item type is populated.
