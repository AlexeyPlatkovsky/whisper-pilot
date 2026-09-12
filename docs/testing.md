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
| Unit (Rust) | Audio decode validation, timestamp math, bounded queues/windows, model-cache scheduling, M2 merge, error mapping | `cargo test` (`npm run test:api`) |
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

## Resource Regression Budgets

Phase 1 uses deterministic payload accounting for the two-hour, 16 kHz mono
boundary, plus opt-in real-process measurements. The synthetic input contains
115,200,000 samples. These figures exclude model weights and allocator/runtime
overhead, so they are an audio-buffer budget rather than a claim about total
RSS:

- direct s16le conversion peaks at one 219.73 MiB PCM buffer plus one 439.45
  MiB f32 buffer (659.18 MiB), below the 700 MiB payload target and 25% below
  the previous PCM + synthetic WAV + f32 path (878.91 MiB);
- blocking Meeting transcription retains one 439.45 MiB f32 allocation, not a
  second full clone;
- 90%-overlap segmentation would eagerly materialize 7,191 windows (4,389.04
  MiB); the 32-window iterator caps its input batch at 19.53 MiB, including the
  tested zero-padded tail;
- diarization transport serialization and deserialization add at most a 16 KiB
  byte buffer instead of another 439.45 MiB byte vector.

Streaming regressions additionally cover backend-owned lifecycle hydration,
Stop during asynchronous `starting`, 100 ms contiguous-pause VAD boundaries,
short meaningful prefixes before capture gaps, terminal cloud-gap ordering,
model-mutation/path-resolution ordering, and atomic cancellation of stale
translations after either a toggle-off or source revision.

Recorder regressions cover native-rate mono PCM16 CAF headers and sample
conversion, one-second durability checkpoints, tail-before-audio-before-database
finalization, interrupted-session reconciliation, owned-audio deletion and WAV
export, shortcut replacement/conflict/repeat policy, backend lifecycle hydration,
and the committed-versus-partial transcript UI. The real default-microphone test
is ignored by default because it requires explicit macOS TCC approval and audible
input; it must be run with the real-Metal gate before Recorder release evidence is
complete.

Floating-bubble regressions cover the five-point click/drag boundary, visible
work-area clamping, removed-monitor fallback, non-color status presentation and
least-privilege capability. WKWebView/native window behavior additionally needs
a packaged-app pass for Retina/mixed-scale displays, monitor removal, Spaces,
Stage Manager, fullscreen, sleep/wake, VoiceOver, Increase Contrast, Reduce
Motion, keyboard restore and capture-status changes while main is hidden.

Local text-model qualification uses the frozen synthetic corpus at
`src-tauri/tests/fixtures/llm_profile_corpus.json` and the reproducible
`scripts/benchmark-llm-profiles.sh` runner. A candidate is promoted only after
real-Metal RU/EN/mixed translation, Streaming and Recorder polishing, short and
long MFU schema, protected-number/identifier, language, reasoning-token and
malformed-output gates pass. The recorded Phase 3 hardware results live in
`docs/validation/phase-3-llm-qualification.md`; publisher benchmarks are not
substitutes for those runs.

The model-free contracts live in `audio.rs`, `commands/transcription.rs`,
`diarize/segmentation.rs`, `diarize_process/transport.rs`,
`streaming_audio.rs`, `streaming_session.rs`, and
`tests/llm_runtime_contract.rs`. Real-Metal and real diarization runs remain
the authority for end-to-end quality and total process RSS.

## Environments

- **Local:** the full `cargo test` + Vitest suites; the ignored pipeline test on
  demand.
- **CI:** hermetic levels only (unit, component, typecheck, build/lint/format).
  CI does not download models or run the ignored pipeline test.

## Quality Gates

Blocking before completion: `cargo build`/`clippy` (zero warnings), `cargo fmt
--check`, `cargo test`, `npm run typecheck`, the Vitest suite, and the
`task-quality` smoke checklist. Run them via `.claude/skills/validate/SKILL.md`.
