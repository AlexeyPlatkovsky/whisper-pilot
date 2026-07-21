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
- during execution
- after a decision is already confirmed
- for purely factual questions with no trade-offs

## Rules

### 1. One Question at a Time

Ask exactly one question per turn. Do not bundle questions. Do not ask a follow-up in the same message.

### 2. Always Provide Options

For every question, provide 2–3 concrete, comparable options. Options must be distinct, actionable, and specific enough to compare. Always include a free-form path so the user can supply their own answer when listed options are incomplete.

### 3. Always Highlight Trade-Offs

State what each option optimizes for, what it sacrifices, and what risks it carries. Do not present options as equally valid when one is materially stronger given the project's constraints.

### 4. The User Decides

The AI presents options and trade-offs. The user makes the decision. Do not assume a decision was made implicitly.

### 5. Stop and Wait

After asking the question, explicitly ask the user to choose or clarify. Stop. Wait for input before moving on.

### 6. Never Mix Brainstorming with Execution

During brainstorming: do not create files, do not edit instructions, do not implement. Brainstorming produces decisions, not artifacts.

### 7. Focus on High-Impact Decisions

Ask only about decisions that materially affect routing, orchestration, validation, structure, reusable documentation, or capability triggers. This includes both feature-level design choices (audio pipeline, IPC model, storage strategy) and instruction-system changes. Skip questions whose answer has no downstream effect on the project's architecture, implementation path, or operational contracts.

## Output Contract

At the end of a brainstorming phase, produce a decision summary:
- each decision made
- the selected option
- any caveats or constraints noted by the user

Execution may begin only after the user confirms the summary.

The summary must begin with:

`Skill: brainstorm - output below`
