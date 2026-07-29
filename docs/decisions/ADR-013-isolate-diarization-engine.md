# ADR-013: Isolate the diarization engine in a child process

- **Status:** accepted
- **Date:** 2026-07-26
- **Deciders:** Alexey Platkovsky

## Context

sherpa-onnx's vendored C++ fast-clustering previously aborted the whole
process with SIGBUS on some real recording + embedding-model + threshold
combinations. WP-62 removes that clustering implementation from the production
path, but diarization still invokes native ONNX Runtime and embedding-extractor
code. A fatal OS signal is not a catchable Rust `Err` or panic, so it can bypass
`apply_diarization_outcome`'s fail-open contract entirely — the app dies
mid-run rather than degrading to speaker-less segments.

This was first recorded (WP-50) as a threshold-only risk, with 0.9 believed to
sit safely below a 0.94 boundary. That boundary was measured with CAM++ alone.
A later triage (WP-53) reproduced a deterministic crash at the shipped 0.9
using TitaNet-large, the catalog's *recommended* embedding, so the shipped
value sits above the boundary for the model most users run.

Tuning cannot resolve it. The two constraints point in opposite directions: on
the 92.4s recording crash safety needs threshold < 0.8, while on the
known-2-speaker 861.5s recording separation quality needs 0.9 — at 0.7 the
second real speaker splits in two and cluster count rises 7 → 12. ADR-005
already documents that clustering quality is input-dependent; this adds that
its *stability* is too.

## Decision

Run the diarization engine call in a **child process**. WP-62 preserves this
containment after replacing vendored clustering; the user selected the measured
0.85 automatic clustering threshold.

The child is this same binary re-executed with a hidden argv flag
(`--wp-diarize-worker`), not a separate sidecar binary, so the app keeps one
binary and one code signature. The parent classifies four child outcomes —
clean exit with payload, exit by signal, non-zero exit, inactivity kill — and
maps every failure onto the existing `AppError::Diarization` fail-open path.

Supervision uses an **inactivity** budget rather than a total wall-clock one,
driven by direct segmentation-batch and embedding-extraction progress, so it
does not scale with recording length. Killing the child remains the only
cancellation mechanism available.

## Consequences

- A native abort now costs the speaker labels instead of the whole app; combined
  with WP-54's persist-before-diarize ordering, the transcript is already safe.
- Removing the known clustering abort does not make native inference failures
  catchable, so the isolation must not be removed on the grounds that the
  replacement path completed the initial recordings.
- A second process holds its own copy of the samples (~55MB for the longest test
  recording) and its own loaded models for the duration of the pass.
- Samples cross as a file under `<app-support>/cache/diarize` rather than a
  pipe, which would need concurrent write-and-read handling to avoid filling the
  pipe buffer.
- The child must be told where its dylibs are: the binary carries no `LC_RPATH`,
  so `DYLD_FALLBACK_LIBRARY_PATH` is set explicitly rather than inherited from
  however the parent happened to be launched. *(No longer the only mechanism:
  WP-60 gave the binary its own rpaths and bundles the dylibs into
  `Contents/Frameworks`. Setting the fallback explicitly still stands.)*
- The child watches stdin for EOF so quitting the app stops it, rather than
  leaving it reparented and still running inference. It terminates with
  `libc::_exit` rather than `std::process::exit` (which would run the native
  runtime's static destructors underneath a thread still inside inference) or
  `abort` (which would leave a crash report behind every ordinary quit).
- A *contained* crash still produces a macOS crash report for the worker
  process in `~/Library/Logs/DiagnosticReports`, because the child genuinely
  faults. The app itself survives and the user sees only the
  `diarization_warning`, but the reports accumulate.
- The four outcomes stay distinguishable on purpose: WP-57's embedding-model
  fallback retries a crash but must not retry a timeout.

## Alternatives Considered

- **Lower the threshold below the crash boundary** — trades the crash for worse
  separation on a recording whose speaker count is known; rejected.
- **Catch the signal in-process** (`SIGBUS` handler) — the faulting C++ leaves
  the allocator and ONNX runtime in an undefined state; resuming from it is not
  sound, and it would not free the models.
- **A separate sidecar binary** — a second executable to build, sign, notarize,
  and keep in sync with the library it links; rejected for a hidden argv mode on
  the one binary that already exists.
- **Total wall-clock timeout** — would have to scale with recording length,
  turning a supervision constant into a guess about input size.
- **Upstream a fix to sherpa-onnx** — worth filing, but does not help this
  release.
