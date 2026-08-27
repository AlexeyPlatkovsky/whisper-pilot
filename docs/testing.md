# Testing

Owns the test strategy and how feature scenarios are executed. Feature-level
scenarios live in TaskPilot (project key WP). Project-wide quality
gates are in `AGENTS.md`.

## Strategy

Quality centers on **transcription and pipeline correctness**, verified without a
live model wherever possible. Pure logic (audio decode validation, the M2
turn↔segment merge, sentence/segment handling) is unit-tested and must hold by
construction. Model-dependent behavior (actual Whisper/diarization output) is
verified by an **ignored, opt-in end-to-end test** that needs a model and ffmpeg
on the machine, so the default suite stays fast and hermetic.

"Tested" for a feature means: its logic has unit tests, its `scenarios.md`
Given/When/Then are covered (automated where feasible, otherwise on the manual
checklist), and the build/lint/format/typecheck gates pass.

## Test Levels

| Level | Scope | Tooling |
| --- | --- | --- |
| Unit (Rust) | Audio decode validation, timestamp math, M2 merge algorithm, error mapping | `cargo test` (`npm run test:api`) |
| End-to-end pipeline | file → ffmpeg → Whisper → segments, on a real model | `cargo test --test pipeline -- --ignored` (needs model + ffmpeg) |
| Unit/Component (front-end) | IPC bindings, transcript state, editing, save | Vitest (`npm run test`) |
| Typecheck | TS ↔ Rust IPC shape agreement | `npm run typecheck` |

The front-end suite grows as the UI does; M1 keeps logic thin and Rust-side.

## Running Feature Scenarios

Each TaskPilot scenario maps to either:

- an **automated** test at one of the levels above (preferred for pure logic and
  the pipeline), or
- a **manual checklist** item run against the built app (for UX states and
  model-quality judgments that cannot be asserted deterministically — e.g.
  "Russian transcript reads accurately").

Two file-handling checks recur across attach/save flows and belong on that
manual checklist wherever such a flow is touched: the file picker filters to
audio/video files and cancelling it is a no-op; a saved file reopens with the
edited transcript text intact.

The manual checklist is run before a TaskPilot item closes, per
`.claude/skills/task-quality/SKILL.md`.

## Coverage Expectations

- Every non-trivial pure function (notably the M2 merge) ships with unit tests
  covering equivalence partitions and boundaries.
- The end-to-end pipeline test must pass on a machine with a model + ffmpeg
  before a transcription-affecting change is considered done.
- Model-quality claims (accuracy, speaker attribution) are evidenced by a
  recorded manual run, not asserted numerically in CI.

## Environments

- **Local:** the full `cargo test` + Vitest suites; the ignored pipeline test on
  demand.
- **CI:** hermetic levels only (unit, component, typecheck, build/lint/format).
  CI does not download models or run the ignored pipeline test.

## Quality Gates

Blocking before completion: `cargo build`/`clippy` (zero warnings), `cargo fmt
--check`, `cargo test`, `npm run typecheck`, the Vitest suite, and the
`task-quality` smoke checklist. Run them via `.claude/skills/validate/SKILL.md`.
