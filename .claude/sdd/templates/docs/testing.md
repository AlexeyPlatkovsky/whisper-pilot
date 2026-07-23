# Testing

<!-- Owns: test strategy and how feature scenarios/checklists are executed. Per-feature
     scenario content lives in features/F<NNN>/scenarios.md. Use only commands and
     tooling verified in the repository; label planned levels as planned. Omit this file
     entirely on the Lean tier. -->

## Strategy

<!-- The overall approach to quality and what "tested" means for this project.
     Link to AGENTS.md for project-wide quality gates rather than repeating them. -->

## Test Levels

<!-- WhisperPilot's current test pyramid. Adjust the rows to match the current suite;
     do not list scripts, CI jobs, or tools that the repository does not provide. -->

| Level | Scope | Tooling |
| --- | --- | --- |
| Unit/Component (front-end) | React UI, local state, and IPC bindings | Vitest + React Testing Library (`npm run test:run`) |
| Unit (Rust) | Rust modules and local persistence/processing logic | `cargo test --manifest-path src-tauri/Cargo.toml` (`npm run test:api`) |
| Typecheck | TypeScript compilation | `npm run typecheck` |
| Lint / format | Front-end static checks and formatting | `npm run lint`, `npm run format:check` |
| Model-backed manual verification | Real local media, ffmpeg, and installed models | Explicit local run; not part of the default automated suite |

## Running Feature Scenarios

<!-- How the Given/When/Then scenarios in features/F<NNN>/scenarios.md are executed:
     automated (which runner/level above) and/or manual (who runs the checklist and when,
     e.g. as part of the smoke checklist in task-quality before an item closes). -->

## Coverage Expectations

<!-- What must be covered before a feature is considered done. State only enforced
     thresholds; if no threshold is configured, define risk-based expectations instead. -->

## Environments

<!-- Where tests run: local and any configured CI workflow. The default suite must not
     depend on network access, model downloads, or cloud APIs. Model-backed checks are
     explicit local verification unless CI configuration proves otherwise. -->

## Quality Gates

<!-- Conditions that block completion. Refer to AGENTS.md and the routed validation
     skill for mandatory gates; list the verified, task-relevant commands selected for
     the change rather than inventing an aggregate validation script. -->
