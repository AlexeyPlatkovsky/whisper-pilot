---
name: discover-requirements
description: Structured Q&A to elicit complete, unambiguous requirements for a feature, epic, or task before implementation begins. Never guesses at unclear points — always asks.
---

# Skill: discover-requirements

## Purpose

Drive a structured, iterative conversation with the user to surface every requirement, edge case, failure state, and constraint for a piece of work before any spec is written.

## When This Skill Applies

Use only when `.claude/pipelines/discover-feature.md` invokes initial or gap-targeted requirements Q&A. Direct invocations outside that pipeline return to `.claude/skills/task-routing/SKILL.md`.

## Context Loading

Before asking any questions:

1. Read the relevant feature requirements, scenarios, and task records under
   `docs/features/`, when they exist.
2. If the requested work touches or depends on existing UI, IPC, Rust core, or database behavior, read the relevant sections of `docs/architecture.md`.
3. State which architecture docs were checked, or "Architecture docs skipped: <reason>."

## Q&A Rounds

Ask questions in rounds. Complete each round fully before starting the next. Do not bundle all rounds into one message.

If this is a **gap-targeted re-entry** after scope-verifier found gaps: use the current draft, gap table, and any new user answers to revise only the affected rounds. Do not restart from Round 1.

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
- Any platform-specific behavior differences between macOS and Windows?

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
  Scenario: <Happy path — one-line title>
    Given ...
    When ...
    Then ...

  Scenario: <Error state — one-line title>
    Given ...
    When ...
    Then ...

  Scenario: <Edge case — one-line title> (add as many as surfaced)
    Given ...
    When ...
    Then ...

Child-task breakdown:
- <required for feature altitude; use "Not applicable — task altitude" for a task>

Child-feature breakdown:
- <required for epic altitude; use "Not applicable — feature/task altitude" otherwise>
```

Apply the fields by altitude:
- **Epic:** replace the entire Proposed BDD scenarios section and Child-task
  breakdown value with `Not applicable — epic altitude`; populate Child-feature
  breakdown. Keep Description, Non-goals, Constraints, and DoD at epic depth.
- **Feature:** replace the illustrative BDD scenario placeholders with
  feature-level scenarios, populate Child-task breakdown, and set Child-feature
  breakdown to `Not applicable — feature altitude`.
- **Task / standalone:** replace the illustrative BDD scenario placeholders
  with exhaustive scenarios; set both child-breakdown fields to their
  task-altitude not-applicable values.

The three BDD blocks in the format are illustrative placeholders, not a
requirement to create exactly three scenarios. Include every scenario surfaced
at the applicable altitude.

For a UI-only task with no deeper logic, the required happy-path BDD scenario may
be the UI smoke path: it states the starting UI state, the functional interaction
or event, and the observable resulting state. It remains a Given/When/Then
scenario and must not be an imperative click script.

Dependencies / integration and Target surfaces remain required at every altitude, with detail calibrated to that altitude.

## Output Contract

When the draft spec is ready, begin the response with:

`Skill: discover-requirements - output below`

Then emit the draft spec in the format above.

Do not emit this artifact until all six rounds are complete and the draft spec is fully populated.
