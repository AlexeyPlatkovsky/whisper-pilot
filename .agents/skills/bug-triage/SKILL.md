---
name: bug-triage
description: Investigate a WhisperPilot defect with an unknown cause and return evidence without fixing it.
---

# Bug triage

## Apply when

Use when the goal is to reproduce, locate, and classify unexpected behavior before a fix. Skip when the root cause and
reproduction are already confirmed or the request includes immediate remediation.

## Boundary

- Remain read-only for production and test sources.
- Do not inspect private media or transcripts unless the user supplies the exact artifact for this purpose.
- Prefer reported errors, focused tests, and a scoped local reproduction.
- Read the relevant `docs/architecture.md` section before concluding a cross-layer cause.

## Investigation

- Record expected and actual behavior, environment, frequency, and reproduction steps.
- Run only checks that can reproduce or isolate the reported behavior.
- Identify the narrowest supported code location and mechanism.
- Distinguish direct evidence from inference and state confidence.
- Stop after three equivalent reproduction attempts or five targeted source searches without new evidence.
- Classify severity by user impact, data risk, workaround availability, and scope.

## Disposition

Choose one: confirmed fix candidate, needs more information, covered known issue, or verified by-design behavior.

## Result

Report reproduction evidence, location, mechanism, confidence, severity, affected layers, disposition, and next input.
