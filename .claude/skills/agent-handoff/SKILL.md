---
name: agent-handoff
description: Select and recover an eligible isolated runner for a routed WhisperPilot agent or subagent handoff.
---

# Skill: agent-handoff

## Purpose

Deliver a routed handoff to a distinct agent without widening its declared tool,
sandbox, or state-mutation boundary.

## Procedure

1. Compare each candidate runner's tool access, sandbox, and state-mutation
   boundary with the invoked agent's declared boundary. A missing, broader, or
   unmatched boundary is ineligible.
2. Pass the invoked agent instructions and every required structured input
   artifact explicitly to each eligible candidate. A runner without them is
   ineligible.
3. Select an eligible native agent-creation or subagent tool when available.
4. If native execution is unavailable, or fails before target action begins, use
   an installed purpose-built isolated runner.
5. Diagnose a failed runner. Retry it once only when no target action began, or
   the attempted work is read-only or idempotent and the cause is understood.
6. Use another eligible runner only when the earlier attempt cannot have caused
   a duplicate or conflicting action.
7. Ask the user only after every eligible safe runner is unavailable or
   exhausted, or when uncertain prior mutation makes another attempt unsafe.

## Output Contract

Begin with:

`Skill: agent-handoff - output below`

| Status | Agent / Handoff | Runner | Result |
|--------|------------------|--------|--------|

`Status` is `completed` or `blocked`.
