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

Required input is a structured request containing: route/run identity; a stable
handoff-invocation ID; handoff sequence number; invoking artifact and scope;
invoked-agent path and full instructions; declared tools, filesystem, network,
sandbox, approval, external-write authority, process authority, allowed targets,
and mutation boundary; expected output label; required input artifacts;
candidate-runner capability records; and prior-attempt history with runner
attempt numbers.
Missing input is `blocked`.

1. Treat an applicable project instruction, pipeline, or routed handoff that
   explicitly names the invoked agent as standing authorization for that
   handoff. Do not ask the user again solely to authorize the invocation.
   Stop when the requested handoff is outside the invoking artifact's scope or
   a higher-level consent gate applies.
2. Compare each candidate runner's tools, filesystem scope, network, sandbox,
   approval mode, external-write authority, processes, and allowed targets with
   the invoked agent's declared boundary. Classify a runner as an exact match
   only when every dimension is equal. Reject a runner that lacks any required
   capability or cannot preserve the state-mutation boundary. Classify a
   sufficient superset as a constrained broader runner only when explicit
   read-only or bounded-tool instructions make it safe for this handoff.
3. Pass the invoked agent instructions, declared boundary, and every required structured input
   artifact explicitly to each eligible candidate. A runner without them is
   ineligible.
4. Select by boundary before runner type: exact native, exact installed,
   constrained native, then constrained installed.
5. If the selected runner is unavailable or fails before target action begins,
   continue through that precedence order subject to the retry rules below.
6. Diagnose a failed runner and inspect any possible target state. Retry only
   when independent evidence proves that no target action or effect occurred,
   or a caller-stable idempotency mechanism prevents duplication. Absence of
   evidence never permits a retry.
7. Use another eligible runner only when the earlier attempt cannot have caused
   a duplicate or conflicting action.
8. Ask the user only after every eligible safe runner is unavailable or
   exhausted, or when uncertain prior mutation makes another attempt unsafe.

## Output Contract

Begin with:

`Skill: agent-handoff - output below`

| Status | Agent / Handoff | Runner | Boundary / Result |
|--------|------------------|--------|--------|

`Status` is `completed`, `blocked`, or
`skipped — invoked agent already isolated`. `completed` requires receipt and
contract validation of the expected labeled agent artifact, regardless of the
agent artifact's own domain verdict. Reserve `blocked` for failure to obtain a
contract-valid artifact and report the agent terminal verdict as
`not available — no contract-valid artifact`.
Use the skipped status only when evidence shows that the current execution
context is already a distinct isolated runner with the invoked instructions,
declared boundary, and every required structured input loaded; cite that
evidence.

Also report the route/run identity, handoff-invocation ID, handoff sequence
number, runner-attempt number, agent terminal verdict, declared boundary, runner
boundary, evidence source, inputs passed, possible prior mutation, expected
artifact, and contract check. Start both counters at `1`. Increment the handoff
sequence for each distinct handoff in the route; increment runner attempt only
for retries of the same handoff-invocation ID. For a broader runner, `Boundary / Result` must state the constraint applied and
cite the runner's available tool record or execution log. Do not claim an
independent proof that no broader capability was used; if no auditable record
exists, state that boundary compliance is an executor attestation. For an
exact-boundary runner, state `exact boundary`.
