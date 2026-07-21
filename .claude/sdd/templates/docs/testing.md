# Testing

<!-- Owns: test strategy and how feature scenarios/checklists are executed.
     Per-feature scenario content lives in features/F<NNN>/scenarios.md.
     Omit this file entirely on the Lean tier. -->

## Strategy

<!-- The overall approach to quality and what "tested" means for this project.
     Link to AGENTS.md for project-wide quality gates rather than repeating them. -->

## Test Levels

<!-- WhisperPilot's test pyramid; adjust rows to match the current suite. -->

| Level | Scope | Tooling |
| --- | --- | --- |
| Unit (front-end) | Stores, plain IPC bindings | Vitest (`npm run test:unit`) |
| Component | React components, hooks, App shell, Bubble overlay | Vitest + React Testing Library (`npm run test:component`) |
| Contract | IPC command name + type-shape contracts (TS ↔ Rust serde) | Vitest (`npm run test:contract`) |
| Design-system | Design-book/token sync, prohibited raw CSS values | Vitest (`npm run test:design-system`) |
| Unit/Integration (Rust) | Audio pipeline, adapters, SessionManager lifecycle | cargo-nextest + tokio + mockall (`npm run test:api`, `cargo nextest run`) |
| End-to-end | Critical paths against the Vite dev build, Tauri APIs mocked | Playwright WebKit (`npm run test:e2e`) |

## Running Feature Scenarios

<!-- How the Given/When/Then scenarios in features/F<NNN>/scenarios.md are executed:
     automated (which runner/level above) and/or manual (who runs the checklist and when,
     e.g. as part of the smoke checklist in task-quality before an item closes). -->

## Coverage Expectations

<!-- What must be covered before a feature is considered done.
     Front-end coverage threshold is enforced at 80% (`npm run test:coverage`). -->

## Environments

<!-- Where tests run: local and CI (.github/workflows/ci.yml, 9 parallel jobs).
     The default suite never calls live OpenAI APIs; live smoke tests are
     environment-gated. -->

## Quality Gates

<!-- Conditions that block completion: `npm run validate`-equivalent checks — lint,
     format, typecheck, Vitest suites, coverage, cargo build/clippy (zero warnings)/
     nextest — plus the task-quality smoke checklist and DoD gate. -->
