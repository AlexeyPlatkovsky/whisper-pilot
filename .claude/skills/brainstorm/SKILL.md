---
name: brainstorm
description: Structured discussion for open design decisions with meaningful trade-offs in the WhisperPilot project.
---

# Skill: brainstorm

## When This Skill Applies

Use when:
- a design decision has multiple valid paths and meaningful trade-offs
- setup or profile clarification requires choosing between meaningful options
- open architecture questions must be resolved before implementation can begin

Do not use:
- after test or production edits have begun
- after a decision is already confirmed
- for purely factual questions with no trade-offs

Require a caller-supplied list of unresolved decisions, approved scope/DoD,
known constraints, and relevant authorities. Use only when at least two viable
choices change a named downstream contract. In an implementation pipeline,
these must be execution choices that do not change readiness, approved scope,
or DoD; route any readiness-blocking decision back through requirements
discovery instead.

Before asking a question, validate that decision IDs are unique and each names
a downstream contract; scope and DoD are non-empty; constraints are listed or
explicitly `none`; and authorities are identified by path/label or explicitly
`none — <reason>`. If any input fails these checks or includes a readiness
blocker, emit the blocked shape below and stop. If constraints leave fewer than
two viable choices, do not invent options: emit the blocked shape and return the
decision to the caller as factual or constrained.

## Rules

### 1. One Question at a Time

Ask exactly one question per turn. Do not bundle questions. Do not ask a follow-up in the same message.

### 2. Always Provide Options

For every question, provide 2–3 concrete, comparable proposed options plus one
free-form choice.

### 3. Always Highlight Trade-Offs

State what each option optimizes for, what it sacrifices, and what risks it carries. Do not present options as equally valid when one is materially stronger given the project's constraints.

### 4. The User Decides

The AI presents options and trade-offs. The user makes the decision. Do not assume a decision was made implicitly.

### 5. Stop and Wait

After asking the question, explicitly ask the user to choose or clarify. Stop. Wait for input before moving on.

### 6. Never Mix Brainstorming with Execution

During brainstorming, do not mutate files or execute implementation.
Brainstorming still produces the required conversation-visible decision
artifact.

### 7. Focus on High-Impact Decisions

Ask only about decisions that materially affect routing, orchestration, validation, structure, reusable documentation, or capability triggers. This includes both feature-level design choices (audio pipeline, IPC model, storage strategy) and instruction-system changes. Skip questions whose answer has no downstream effect on the project's architecture, implementation path, or operational contracts.

## Output Contract

Treat a decision as selected only when the user explicitly names an option or
states an unambiguous free-form choice for its decision ID. A conditional,
changed, or ambiguous answer remains unresolved and must be clarified one
question at a time.

When every caller-supplied decision is resolved, produce a versioned decision
summary:
- one row for each decision made
- the selected option
- any caveats or constraints noted by the user

Execution may begin only after the user confirms the summary.

The summary must begin with:

`Skill: brainstorm - output below`

Then emit:

`Status: awaiting confirmation`, `Status: confirmed`, or `Status: blocked`

`Summary version: <caller-stable route/run ID>:<positive revision number>`

| Decision | Selected Option | Caveats / Confirmation |
|----------|-----------------|-------------------------|

Until the user confirms, write `awaiting user confirmation` in the final
column and do not treat the artifact as a completed decision summary. Ask the
user to confirm the exact summary version. After an explicit confirmation of
that version, state `confirmed by user` in every row and set `Status:
confirmed`. If the user changes any row, increment the revision, return to
`awaiting confirmation`, and require confirmation of the new version.

For `Status: blocked`, emit only the required label, the status, and
`Blocker: <missing/invalid input, readiness routing defect, or ineligible
decision>`. Omit the summary version and decision table.
