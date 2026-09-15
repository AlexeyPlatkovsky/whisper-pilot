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

| Level                      | Scope                                                                                                            | Tooling                                                          |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| Unit (Rust)                | Audio decode validation, timestamp math, bounded queues/windows, model-cache scheduling, M2 merge, error mapping | `cargo test` (`npm run test:api`)                                |
| End-to-end pipeline        | file → ffmpeg → Whisper → segments, on a real model                                                              | `cargo test --test pipeline -- --ignored` (needs model + ffmpeg) |
| Unit/Component (front-end) | IPC bindings, transcript state, editing, save                                                                    | Vitest (`npm run test`)                                          |
| Typecheck                  | TS ↔ Rust IPC shape agreement                                                                                    | `npm run typecheck`                                              |
| Source structure           | Production, test, and architecture-document size boundaries                                                     | `npm run lint:source-size`                                      |

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

The manual checklist is recorded before a TaskPilot item closes, following the
project contract in `AGENTS.md` and the testing workflow in
`.agents/skills/testing/SKILL.md`.

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
- blocking Transcription retains one 439.45 MiB f32 allocation, not a
  second full clone;
- 90%-overlap segmentation would eagerly materialize 7,191 windows (4,389.04
  MiB); the 32-window iterator caps its input batch at 19.53 MiB, including the
  tested zero-padded tail;
- diarization transport serialization and deserialization add at most a 16 KiB
  byte buffer instead of another 439.45 MiB byte vector.

Meeting regressions additionally cover backend-owned lifecycle hydration,
Stop during asynchronous `starting`, 100 ms contiguous-pause VAD boundaries,
background-relative pause thresholds, hard-boundary audio overlap and
timestamp-proven Whisper reconciliation (including intentional repeated
phrases and Qwen's conservative no-deduplication path), fast
partial versus quality commit profiles, 20-second steady-noise suppression,
short meaningful prefixes before
capture gaps, terminal cloud-gap ordering, model-mutation/path-resolution
ordering, first-window Live Translation during active capture, and atomic
cancellation of stale translations after either a toggle-off or source revision.
The local durable-result queue is additionally filled to its fixed 32-window
boundary: all accepted windows remain readable in order, while the next one
returns immediately to the decoder and causes an explicit terminal session
failure instead of unbounded buffering or silent loss.
The Whisper Meeting prompt contract keeps the bilingual punctuation seed
ahead of rolling context. Its release gate is an ignored real-Metal decode of
punctuated speech through the production `WhisperSessionDecoder`; mocked or
text-only tests are not evidence that the model emits punctuation.

Recorder regressions cover native-rate mono PCM16 CAF headers and sample
conversion, one-second durability checkpoints, tail-before-audio-before-database
finalization, interrupted-session reconciliation, owned-audio deletion and WAV
export, shortcut replacement/conflict/repeat policy, backend lifecycle hydration,
the shared searchable library/header, in-place persisted Prettify, confirmed
recording Clear, and the committed-versus-partial
transcript UI. They also prove that a full realtime-ASR queue never blocks the
native-rate writer, and that Recorder errors stay attached to their originating
session and do not leak into Meeting. Recorder and Meeting view tests also
prove that the first saved item opens only after lifecycle hydration. Meeting
UI regressions cover
session-owned On Air state, per-session translation targets, an active MFU
visibility toggle, Original-column partials, and shared MFU overflow scrolling.
They also cover an all-italic provisional source hypothesis, retention of the
last usable italic Cloud translation while a newer preview is pending, local
Whisper partials that do not invoke the text LLM, failed ASR windows that render
unavailable in both columns without entering translation, and confirmed Clear
of transcript, translation, MFU, and Prettify state. Transcription Clear has a
matching derived-content-only persistence contract. `npm run lint:ui` enforces
that confirmation alertdialogs use the shared primitive and its destructive
variant rather than workspace-local modal markup.
Deterministic local-LLM tests use a short injected timeout to prove that one
runtime-owned worker evicts cached weights after idle time, defers eviction
while a scheduler lease is active, keeps eviction atomic with respect to a new
lease, and resets the deadline after completion; the production timeout remains
two minutes.
The real default-microphone test
is ignored by default because it requires explicit macOS TCC approval and audible
input; it must be run with the real-Metal gate before Recorder release evidence is
complete.

ASR engine regressions resolve stable IDs rather than catalog order, use one
selection for every mode, require all files in a model bundle, key caches by
engine/model/fingerprint, normalize removed selections, and persist immutable
Recorder session engine identity. The opt-in real Qwen gate covers the 1.7B
text/projector GGUF pair through the production MTMD decoder when `QWEN3_ASR_GGUF_MODEL`,
`QWEN3_ASR_GGUF_MMPROJ`, and `QWEN3_ASR_GGUF_TEST_AUDIO` are set. The frozen
experimental corpus and retired runtime evidence are recorded in
`docs/validation/phase-4-qwen-asr-qualification.md`; the GGUF runtime has
real-Metal mixed-language smoke evidence, while equivalent
full-corpus WER, latency, and sustained-session evidence remains required
before comparative claims. Unit coverage also pins the upstream prompt shape:
an always-present plain system context (including the empty case) followed by
an audio-only user message, with no extra natural-language transcription
directives.
The synthetic corpus can be regenerated with
`scripts/generate-asr-benchmark-corpus.sh`; `scripts/score-asr-corpus.mjs`
computes Unicode-aware WER/CER from separate hypothesis files and emits metrics
without echoing transcript content. Its normalization and distance contracts
run with `node --test scripts/score-asr-corpus.test.mjs`.

Floating-bubble regressions cover the five-point click/drag boundary, visible
work-area clamping, removed-monitor fallback, non-color status presentation and
least-privilege capability. WKWebView/native window behavior additionally needs
a packaged-app pass for Retina/mixed-scale displays, monitor removal, Spaces,
Stage Manager, fullscreen, sleep/wake, VoiceOver, Increase Contrast, Reduce
Motion, keyboard restore and capture-status changes while main is hidden.

Local text-model qualification uses the frozen synthetic corpus at
`src-tauri/tests/fixtures/llm_profile_corpus.json` and the reproducible
`scripts/benchmark-llm-profiles.sh` runner. A candidate is promoted only after
real-Metal RU/EN/mixed translation, Meeting and Recorder polishing, short and
long MFU schema, protected-number/identifier, language, reasoning-token and
malformed-output gates pass. The recorded Phase 3 hardware results live in
`docs/validation/phase-3-llm-qualification.md`; publisher benchmarks are not
substitutes for those runs.
Translation regression coverage also includes natural cross-script
hyphenation so scientific names are not mistaken for exact identifiers; the
selected Qwen profile is exercised on that case in the ignored real-Metal
corpus test.

Recorder persistence contracts verify that confirmed Clear recording removes
final/partial audio plus raw and polished text, retains an empty reusable
draft, rejects live clearing, and preserves database text when filesystem
cleanup fails. Injected database-failure and restart tests prove quarantine
rollback plus pre-/post-commit crash recovery. Frontend coverage includes
audio-only recordings and the confirmation boundary. Meeting rendering
coverage asserts that the first non-empty partial replaces the centered
Listening placeholder while an empty partial leaves it visible.
Paragraph grouping coverage prefers the next terminal punctuation, keeps a
normal seven-window unfinished sentence together, and bounds a punctuation-free
run at the twelve-window safety ceiling.
Live Translation coverage verifies italic Cloud provisional output,
partial-update coalescing, committed-result replacement, context-free
validation fallback, committed-job priority over queued previews, and
old-completion isolation when the next session uses a different target
language. Backend regressions reject failed or source-revised windows before
inference, cap translation output separately from long-form work, and prove the
local capture queue accepts 600 nominal 100 ms chunks before dropping the
601st without blocking.
Race coverage also proves Clear is unavailable during derived-content work,
pending Transcription autosaves settle before Clear without cancelling an edit queued
for another meeting or leaking its failure into the newly opened meeting, a
cleared Recorder/Meeting row rejects late polish/MFU persistence, a
cleared-then-resumed Meeting duration follows `end_ms` instead of wall-clock
age, and preview A cannot attach to partial B after A crosses a commit boundary.

Recorder continuation contracts keep the prior finalized CAF unchanged until
atomic replacement, reject sample-rate mismatch without mutation, append new
PCM to the old timeline, and drive final duration/quality reads from the
combined audio. Frontend coverage asserts completed-row continuation plus
Space/Return confirmation, Escape cancellation with focus restoration, and
modal Tab trapping for delete.

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
--check`, `cargo test`, `npm run typecheck`, the Vitest suite, and the applicable
TaskPilot/manual smoke checklist. `npm run lint:source-size` rejects a production
module as soon as it reaches 750 lines, documentation over 600 lines, and
front-end or Rust test modules as soon as they reach 1,750 lines. It also covers repository
scripts, workflows, root documentation, and the Rust build script. The staged
form runs in pre-commit; the full gate and its tests run again in CI together
with AI-instruction validation. The only temporary exception is
`cloud_streaming.rs`, fixed at its 935-line WP-130 baseline and rejected if it
grows. Code review must require a coherent ownership boundary, not a mechanical
line move, when a module approaches the limit. The authoritative command list
is in `docs/development.md`.
