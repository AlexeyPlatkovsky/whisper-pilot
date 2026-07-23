# F<NNN> <Feature Name> — Scenarios

<!-- Behavior verification for the feature: Gherkin scenarios plus a manual checklist.
     Each scenario traces to one or more requirement IDs. Cover success, relevant error,
     and boundary states; do not write unverifiable implementation steps. -->

## Scenarios

### F<NNN>-S1: <scenario name>

**Covers:** `F<NNN>-R1`

```gherkin
Scenario: <scenario name>
  Given <initial context>
    And <additional context>
  When <action>
  Then <expected outcome>
    And <additional outcome>
```

## Manual Verification Checklist

<!-- For checks not easily automated. Each item must be observable, name the environment
     or fixture needed when material, and link to a requirement. -->

- [ ] (F<NNN>-R1) <observable check>
- [ ] <accessibility / error-state / edge-case check>
