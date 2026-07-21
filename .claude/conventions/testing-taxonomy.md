# testing-taxonomy.md

## Purpose

Single authoritative source for the project test taxonomy: level definitions, scope boundaries, tool assignments, directory conventions, and quality practice standards. All instruction artifacts reference this convention; none duplicate its content.

## Test Pyramid and Levels

All test code must target the correct level per the following taxonomy:

| Level | Scope | Tools | Directory |
|-------|-------|-------|-----------|
| **Unit** | Pure functions, reducers, helpers | Vitest / cargo-nextest | `src/**/*.test.{ts,tsx}`, `#[cfg(test)]` |
| **Component** | Single React component with mocked IPC | Vitest + RTL + userEvent | `src/components/*.test.tsx` |
| **Integration** | Store + registry pipeline, Tauri command layer | cargo-nextest (Rust) | `src-tauri/tests/` |
| **Contract** | IPC shape validation, round-trip serde | Vitest / cargo test | `src/chat/contract.test.ts`, Rust inline |
| **E2E** | Full UI in WebKit engine | Playwright WebKit | `e2e/*.spec.ts` |
| **A11y** | Accessibility axe audits | jest-axe / @axe-core/playwright | `*.a11y.test.tsx`, `e2e/a11y.spec.ts` |
| **Property** | Randomized invariant verification | fast-check / proptest | `*.proptest.ts`, Rust inline |

## Additional Quality Practices

- **Coverage thresholds** enforced via `@vitest/coverage-v8` (80% lines, branches, functions, statements).
- **Dependency auditing** via `npm audit --production` and `cargo audit`.
- **Smoke tests** tag critical-path tests with `[smoke]` (Vitest) or `@smoke` (Playwright).
- **Property-based testing** for parsing, stripping, and reduction functions. Use `fast-check` (TS) and `proptest` (Rust).
- **Accessibility testing** via `jest-axe` for components and `@axe-core/playwright` for E2E.
- **Mutation testing** validates test quality on `feature/*` branches. Stryker-js + cargo-mutants.
- **Contract tests** verify round-trip serialization for all IPC types and that generated TypeScript bindings match the Rust source of truth.
- **Static analysis** uses `typescript-eslint stylisticTypeChecked` rules, `eslint-plugin-testing-library`, and `clippy::pedantic`.

## Selection Heuristics

| What's being tested | Level | Example location |
|---------------------|-------|-----------------|
| Pure function (parser, reducer, helper) | Unit | `src/state/chat.test.ts`, `#[cfg(test)]` |
| Single React component | Component | `src/components/Bubble.test.tsx` |
| Store + adapter + routing pipeline | Integration | `src-tauri/tests/integration_store_routing.rs` |
| IPC type round-trip | Contract | `src/chat/contract.test.ts` |
| Full UI in WebKit | E2E | `e2e/composer.spec.ts` |
| Accessibility audit | A11y | `src/components/Bubble.a11y.test.tsx` |
| Invariant over random inputs | Property | `src/**/*.proptest.ts` |

## Test-Design Techniques (Case Derivation)

The pyramid above is the **level** axis — *where* a test runs. This section is the **case-derivation** axis — *which* cases a test must cover. The two are orthogonal: pick the level from the taxonomy, then derive the cases for that level with the technique(s) below. Deriving cases by these techniques — rather than by intuition — is how edge cases and negative scenarios stop being accidental.

Every technique must produce **negative and invalid cases**, not only valid ones. A suite that exercises only the happy partition does not satisfy this convention.

| Technique | Derive cases when | Cases it forces | Project example |
|-----------|-------------------|-----------------|-----------------|
| **Equivalence Partitioning (EP)** | An input has distinguishable classes that should each be handled the same way | One representative per valid class **and** per invalid class | Provider reasoning value: classes `empty` / `"none"` / `arbitrary non-empty` → each handled distinctly (no flag vs passed-through arg) |
| **Boundary Value Analysis (BVA)** | An input is ordered — numeric, length, count, or size | At, just below, and just above each boundary (min−1, min, max, max+1) | Composer auto-grow height: at the 32px min-height, and just-below/at/just-above the 96px max-height scroll threshold; prompt length empty vs single-char vs very long |
| **Decision Table** | Behavior depends on a **combination** of independent conditions | Enumerate all 2ⁿ condition combinations; mark impossible ones explicitly as unreachable (documented, not tested); one test per reachable rule | Preference fallback (SP-036): file-valid × per-provider-valid/invalid → which providers keep settings vs use Rust defaults; CLI arg construction from model × reasoning × provider |
| **State-Transition** | Behavior is **stateful** — events move the system between states | Valid transitions, invalid events per state, and guard conditions | Prompt lifecycle: in-flight vs idle; an in-flight prompt keeps its config snapshot when preferences change mid-flight; panel open/collapsed; session lifecycle |
| **Pairwise / Combinatorial** | ≥3 independent multi-valued parameters make full coverage explode | Every pair of parameter values covered (pairwise is the coverage goal, not a fixed case count; no tool is mandated) | Provider × model × reasoning × platform (macOS/Windows) matrices |

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
- **E2E budget.** The budget is a justification test, not a numeric cap. E2E (Playwright WebKit) is the scarcest level — slowest and most brittle — and is reserved for **critical-path and cross-engine behavior that no lower level can cover**: end-to-end routing through the real window, platform/WebKit-specific rendering, and `[smoke]`/`@smoke`-tagged critical paths. New E2E coverage must justify why the scenario cannot be pushed down. Duplicating a lower-level assertion at E2E is not added coverage — it is budget spent for no gain.
- **Runtime UI validation is not E2E.** The runtime-UI-validation gate (`AGENTS.md` §Quality Gates) requires real-window evidence for UI changes; satisfying it does not require a standing E2E spec. Prefer pushing the *assertion* down while supplying runtime evidence separately.
