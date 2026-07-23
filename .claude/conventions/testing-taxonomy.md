# testing-taxonomy.md

## Purpose

Single authoritative source for the project test taxonomy: level definitions, scope boundaries, tool assignments, directory conventions, and quality practice standards. All instruction artifacts reference this convention; none duplicate its content.

## Test Pyramid and Levels

All test code must target the correct level per the following taxonomy:

| Level | Scope | Tools | Directory |
|-------|-------|-------|-----------|
| **Unit** | Pure TypeScript functions or isolated Rust logic | Vitest / `cargo test` | `src/**/*.test.{ts,tsx}`, Rust `#[cfg(test)]` |
| **Component** | A React component with the IPC boundary mocked | Vitest + RTL + userEvent | `src/*.test.tsx` or next to the component |
| **Integration** | Multiple Rust modules, storage, or transcription/diarization pipeline seams | `cargo test` | `src-tauri/tests/` |
| **Contract** | Rust command DTO serialization and the matching `src/ipc.ts` wrapper shape | Vitest / `cargo test` | Rust inline or matching front-end test |
| **Runtime UI verification** | Window, WKWebView, layout, or platform behavior that jsdom cannot execute | Manual Tauri macOS verification | Evidence artifact, not a standing test directory |
| **Property** | Randomized invariant verification when a suitable library is explicitly added | fast-check / proptest | Close to the tested function |

## Additional Quality Practices

- **Coverage thresholds, mutation testing, E2E, property testing, and axe audits are not configured project-wide.** Add them only through a routed change that adds the dependency, command, and ownership; do not claim them as existing gates.
- **Accessibility** is tested through accessible RTL queries for component behavior and manual real-window verification where WKWebView behavior matters.
- **Contract tests** verify observable Rust command DTO behavior and consistency with the typed wrappers in `src/ipc.ts`; this project does not currently generate bindings.
- **Static analysis** uses the commands actually defined by the repository (`npm run lint`, `npm run typecheck`, `npm run format:check`, and relevant Cargo checks). Do not require an unconfigured linter mode.

## Runtime UI verification evidence

Runtime UI verification is a task-local evidence artifact, not a test directory. The routed
pipeline records it in the visible `Manual UI verification record`, passes it to
`test-runner`, and cites it in the TaskPilot completion evidence. The record uses this format:

`Status` — exactly one of `Pass`, `Fail`, or `External verification limitation`.

**Environment** — macOS version, Safari/WebKit version for WKWebView-sensitive behavior, and
app build mode.

| State / interaction | Expected result | Observed result | Result |
| --- | --- | --- | --- |

For an external verification limitation, also record `Scope`, `Cause`, `Unavailable Coverage`,
and `Implementation Defect Found` (`no`). A `Pass` requires all relevant rows to pass; a `Fail`
must identify the implementation defect.

## Selection Heuristics

| What's being tested | Level | Example location |
|---------------------|-------|-----------------|
| Pure function (formatting, i18n, theme, speaker label) | Unit | `src/*.test.ts`, Rust `#[cfg(test)]` |
| Single React screen or section | Component | `src/*Screen.test.tsx`, `src/*Section.test.tsx` |
| Meeting storage or transcription/diarization pipeline seam | Integration | `src-tauri/tests/*.rs` |
| IPC DTO and wrapper agreement | Contract | Rust inline or the matching `src/ipc.ts` consumer test |
| Window/WebKit-specific behavior | Runtime UI verification | Manual Tauri macOS evidence |
| Invariant over random inputs | Property | Only after the task configures fast-check or proptest |

## Test-Design Techniques (Case Derivation)

The pyramid above is the **level** axis — *where* a test runs. This section is the **case-derivation** axis — *which* cases a test must cover. The two are orthogonal: pick the level from the taxonomy, then derive the cases for that level with the technique(s) below. Deriving cases by these techniques — rather than by intuition — is how edge cases and negative scenarios stop being accidental.

Every technique must produce **negative and invalid cases**, not only valid ones. A suite that exercises only the happy partition does not satisfy this convention.

| Technique | Derive cases when | Cases it forces | Project example |
|-----------|-------------------|-----------------|-----------------|
| **Equivalence Partitioning (EP)** | An input has distinguishable classes that should each be handled the same way | One representative per valid class **and** per invalid class | Meeting title: empty / whitespace / valid non-empty / over limit |
| **Boundary Value Analysis (BVA)** | An input is ordered — numeric, length, count, or size | At, just below, and just above each boundary (min−1, min, max, max+1) | Meeting-title limit or timestamp boundaries |
| **Decision Table** | Behavior depends on a **combination** of independent conditions | Enumerate all 2ⁿ condition combinations; mark impossible ones explicitly as unreachable (documented, not tested); one test per reachable rule | Model availability × selected task × download state |
| **State-Transition** | Behavior is **stateful** — events move the system between states | Valid transitions, invalid events per state, and guard conditions | Idle → transcribing → completed/error; meeting open/delete/create lifecycle |
| **Pairwise / Combinatorial** | ≥3 independent multi-valued parameters make full coverage explode | Every pair of parameter values covered (pairwise is the coverage goal, not a fixed case count; no tool is mandated) | Input media format × language × model availability |

### Application Rules

- **Match rigor to risk.** Apply combinatorial, decision-table, and state-transition coverage to high-risk areas — route planning, CLI argument construction, persistence/serialization, and IPC contracts. A single representative per partition may suffice for a low-risk pure helper. Do not spend a decision table on trivial display logic.
- **Combine techniques** when an input is both classed and ordered (EP + BVA) or stateful and conditional (state-transition + decision table). They are not mutually exclusive.
- **Make the derivation visible.** A test case derived by one of these techniques should name that technique, so coverage of the case space is auditable rather than incidental — e.g. an inline comment `// EP: invalid reasoning class`, `// BVA: max-height + 1px`, or a Gherkin tag (`@decision-table`, `@bva`) on the scenario. The goal is that a reviewer can see *why* the chosen cases are sufficient; review is where this is checked.
- **Decision tables and state models belong in the spec.** When a feature's behavior is driven by a decision table or a state machine, capture that table or diagram in the TaskPilot item's description or a durable project reference document so test cases trace back to an enumerated source, not to the author's memory.

## Spec-to-Test Traceability

This section governs **requirements coverage** — the link between a feature's specified behavior and the tests that verify it. It is distinct from `.manifesto/conventions/traceability.md`, which governs *transcript* auditability of routed instruction-system steps; that convention is about agent-execution artifacts, this one is about test ↔ requirement coverage. The two do not overlap.

Traceability is **bidirectional**:

- **Forward (spec → test).** Every relevant DoD bullet in the TaskPilot item's `dod` and every `Scenario:` in its description is covered by at least one test. A scenario with no test is an uncovered requirement, not an optional one; the review layer checks this.
- **Backward (test → spec).** Every behavioral test must trace to a scenario, an acceptance criterion, or — for non-behavioral work — an explicitly recorded invariant. A test that maps to nothing is dead, mis-scoped, or testing unspecified behavior; investigate rather than leave it.

Make the link **visible and grep-able** so coverage is auditable, not assumed:

- Tag each Gherkin scenario with a stable id (e.g. `@SP-036-fallback-partial`) and reference that id in the covering test's name or a comment, so a single `grep` ties scenario to test.
- Or name the test after the scenario title.
- Property, contract, and a11y tests trace to the invariant or criterion they enforce, not to a Gherkin scenario.

A single scenario may be covered by tests at more than one level (e.g. a unit test plus a component test); cite the scenario id in each. This is the standard the review layer checks; it does not gate from this convention.

## Coverage Placement (Push-Down + E2E Budget)

Traceability says *that* a scenario is covered; this rule says *at which level*. Choose the level deliberately, biased toward the bottom of the pyramid:

- **Push down.** Cover a behavior at the **lowest level that can actually exercise it.** If a rule can be verified by a unit test over a pure function, do not promote it to a component test; if a component test with mocked IPC suffices, do not promote it to E2E. A higher-level test is justified only by behavior the lower level genuinely cannot reach (real engine rendering, cross-process wiring, real WebKit quirks).
- **Runtime UI budget.** Apply real-window verification whenever the selected runtime-UI scope
  cannot be exercised below the window. The highest-priority, non-exhaustive examples are
  file-dialog handoff, native-window layout, and macOS rendering. It is evidence, not an
  unconfigured E2E suite. Prefer pushing behavioral assertions down while supplying runtime
  evidence separately.
