---
name: requirements-discovery
description: Clarify an incomplete feature or task before implementation and produce an approval-ready specification.
---

# Requirements discovery

## Apply when

Use when requested behavior, boundaries, failure handling, or acceptance criteria are materially unclear. Skip when an
approved, implementation-ready TaskPilot record already covers the requested scope.

## Outcome

Produce a concise specification that another agent can implement without inventing product behavior.

## Policy

- Read the existing TaskPilot item, its parent, and the relevant product documentation first.
- Ask only questions whose answers change behavior, scope, risk, or acceptance.
- Ask one focused question at a time and offer concrete alternatives when useful.
- Distinguish confirmed facts, user decisions, repository evidence, and unresolved assumptions.
- Cover the user goal, scope exclusions, happy path, errors, edge boundaries, dependencies, and affected surfaces.
- State acceptance criteria as observable outcomes.
- Propose child work only when parts can be delivered and verified independently.
- Do not edit product code or persist the draft before user approval.

## Result

Return the specification, unresolved questions, proposed TaskPilot fields, and readiness verdict. Use the
`requirements-reviewer` agent for an independent completeness check before persistence.
