# F<NNN> <Feature Name> — Scenarios

<!-- Behavior verification for the feature: Gherkin scenarios + a manual checklist.
     Each scenario traces to one or more requirement IDs. -->

## Scenarios

### F<NNN>-S1: <scenario name>

Covers: F<NNN>-R1

```gherkin
Scenario: <scenario name>
  Given <initial context>
    And <additional context>
  When <action>
  Then <expected outcome>
    And <additional outcome>
```

## Manual Verification Checklist

<!-- For checks not easily automated. Each item should be observable and link to a requirement. -->

- [ ] (F<NNN>-R1) <observable check>
- [ ] <accessibility / error-state / edge-case check>
