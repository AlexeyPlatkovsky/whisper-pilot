---
name: agent-handoff
description: Select and recover an eligible isolated runner for a routed WhisperPilot agent or subagent handoff.
---

# Skill: agent-handoff

## Purpose

Deliver a routed handoff to a distinct agent without widening its declared
state-mutation boundary. Prefer a runner whose tool and sandbox boundary exactly
matches the invoked agent. A broader runner is eligible only when it can be
explicitly instructed to use no capability beyond the invoked agent's declared
boundary; record that constrained-runner exception in the output.

## Procedure

1. Treat an applicable project instruction, pipeline, or routed handoff that
   explicitly names the invoked agent as standing authorization for that
   handoff. Do not ask the user again solely to authorize the invocation.
   Stop when the requested handoff is outside the invoking artifact's scope or
   a higher-level consent gate applies.
2. Compare each candidate runner's tool access, sandbox, and state-mutation
   boundary with the invoked agent's declared boundary. Reject any runner that
   cannot preserve the state-mutation boundary. Prefer an exact tool/sandbox
   match; otherwise document the explicit read-only or bounded-tool instruction
   that makes a broader runner safe for this handoff.
3. Pass the invoked agent instructions, declared boundary, and every required structured input
   artifact explicitly to each eligible candidate. A runner without them is
   ineligible.
4. Select an eligible native agent-creation or subagent tool when available.
5. If native execution is unavailable, or fails before target action begins, use
   an installed purpose-built isolated runner.
6. Diagnose a failed runner. Retry it once only when no target action began, or
   the attempted work is read-only or idempotent and the cause is understood.
7. Use another eligible runner only when the earlier attempt cannot have caused
   a duplicate or conflicting action.
8. Ask the user only after every eligible safe runner is unavailable or
   exhausted, or when uncertain prior mutation makes another attempt unsafe.

## Output Contract

Begin with:

`Skill: agent-handoff - output below`

| Status | Agent / Handoff | Runner | Boundary / Result |
|--------|------------------|--------|--------|

`Status` is `completed` or `blocked`.

For a broader runner, `Boundary / Result` must state the constraint applied and
cite the runner's available tool record or execution log. Do not claim an
independent proof that no broader capability was used; if no auditable record
exists, state that boundary compliance is an executor attestation. For an
exact-boundary runner, state `exact boundary`.
