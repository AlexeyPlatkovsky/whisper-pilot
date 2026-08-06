# WhisperPilot Architecture

Technical architecture for the offline meeting-notes workspace. Product scope is
owned by `docs/idea.md`; UX by `docs/design.md`. This document owns the layer
map, pipeline, meeting/data model, IPC contract, models, and build notes. The
core unit is a **Meeting** (one transcription of one source file).

## Layer Map

```
React UI (src/)  ──Tauri IPC──▶  Rust core (src-tauri/src/)
  meetings list                    lib.rs        command + event registration, AppState
  meeting workspace                audio.rs      ffmpeg normalize + WAV decode
  transcript editor                transcribe.rs whisper (Metal) full-file decode, progress
  MFU panel                        store.rs      SQLite meeting library (meetings, segments, notes)
  settings screen                  meetings.rs   create/list/open/rename/delete meeting commands
  ipc.ts / events                  export.rs     meeting → Markdown / plain text
  theming / i18n                   error.rs      AppError → serialized to JS
                                   settings.rs   key–value settings store (theme, ui_language, active models)
                                   models.rs     model catalog: download + SHA verify + delete
                                   [M2] diarize.rs   sherpa-onnx speaker turns + merge
                                   [M3] notes.rs     llama.cpp structured meeting notes
                                   [WP-68] streaming_audio.rs  mic + system-audio capture/mix (see below)
```

The Rust core does all heavy work and owns persistence; the React layer is a
two-pane shell — a meetings list plus the active meeting's workspace. Blocking,
CPU/GPU-heavy work (model load,
transcription, diarization, note generation) runs on `tokio::task::spawn_blocking`
so IPC and the UI stay responsive. Long operations report progress and are
cancellable via Tauri events.

## Meeting Model & Persistence (`store.rs`)

The library is a local **SQLite** database (`whisperpilot.sqlite3` in the app
support directory, via bundled `rusqlite`). WP-16 implements the idempotent
schema and Rust CRUD store. WP-21/WP-22 expose create/list/open/rename/delete
commands and hydrate the workspace from persisted meetings; attaching files,
transcription, and UI auto-save wiring remain follow-on work. A **meeting** is
one transcription of one source file. Meetings
**reference the original file path** — audio is not copied — so a meeting whose
source has moved or been deleted is readable but cannot be re-transcribed (a
defined "source missing" state).

Entities (indicative):

| Entity | Key fields |
|---|---|
| `meetings` | id, title, source_path, source_name, created_at_ms, duration_ms, language, status |
| `segments` | meeting_id + ordinal (composite key), start_ms, end_ms, text, speaker_id (M2) |
| `notes` | meeting_id, summary, decisions, action_items, open_questions, participants (M3) |

The store replaces/reads segments in ordinal order, upserts a single notes
record per meeting, and cascades meeting deletion to dependent rows. Follow-on
UI edits (segment text, speaker labels, notes) are **auto-saved**: each edit
will persist to the DB immediately; there is no explicit save state. Export is
a separate, explicit write to an external file.

Stored `segments` rows stay whisper's original fine-grained spans — `to_dto`
(`meetings.rs`, WP-48) coalesces consecutive same-speaker rows into larger
display blocks on every read path (see Speaker Diarization below); the
ordinal-keyed rows themselves are never merged or rewritten. **Not yet
designed:** WP-17's auto-save wiring will edit whatever the UI renders, which
after WP-48 is a coalesced block that can span multiple underlying
ordinal-keyed rows — the write-back mapping from an edited coalesced block
back to its source row(s) still needs a design before WP-17 is implemented.

## Audio Ingestion (`audio.rs`)

Any input — audio or video — is normalized through **one** path: ffmpeg produces
a temporary 16 kHz mono WAV (`-vn -ac 1 -ar 16000`), decoded with `hound` into
f32 samples. ffmpeg extracts audio from video and resamples audio identically, so
no branch on file type is needed. The temporary WAV is deleted after decoding.
ffmpeg is a required external dependency (system binary on PATH for now).

## Transcription (`transcribe.rs`)

whisper-rs with the `metal` feature; the context is created once and cached in
`AppState`. Decoding is **full-file** with beam search. **Language is always
auto-detected and can never be chosen** (ADR-012): `transcribe()` takes no
language argument, so no caller can force one. The code Whisper decoded with is
read back from decoder state and stored on the meeting, making
`meetings.language` an *output* of a run rather than an input to one. Detection
uses the first 30 seconds of audio, so a recording that opens with silence can
misdetect — a known limitation. Whisper's progress callback drives a progress
event; a cancel flag checked in the callback aborts a run. Output is timestamped
`Segment`s persisted to the meeting.

Forcing a language is deliberately unreachable rather than merely defaulted:
decoding audio as a language it is not in makes Whisper emit one hallucinated
line per 30-second window instead of the transcript. `DecodeSettings` names the
configuration as plain data so it can be asserted in a unit test — notably
`detect_language_only`, which must stay `false` because whisper.cpp returns
immediately after detection when it is set, yielding an empty transcript.

The **Transcribe** run is a two-phase pipeline: transcription, then **diarization
+ merge** (M2, `diarize.rs`). The transcript is persisted between the two phases,
so the meeting is already marked finished while diarization is still running (see
Speaker Diarization below). Progress and Stop span both phases; if diarization is
unavailable or fails, the run still finishes with plain (speaker-less) segments.
Re-running Transcribe on a meeting that already has a transcript replaces it (and
any notes) after a confirmation.

The model is the `large-v3-turbo` artifact downloaded and SHA-verified via the
Settings AI models section (F005, `models.rs`) into the app support directory;
override the path for development with `WHISPERPILOT_MODEL_PATH`.

## Speaker Diarization (M2, `diarize.rs`)

sherpa-onnx segmentation + embedding models produce speaker turns, merged onto
segments by time overlap to set each segment's `speaker_id`. Speaker count is
auto-detected with an optional override. Labels are generic ("Speaker N",
English per ADR-011) and user-renamed within the current session. The
transcript renders as a per-speaker chat of colored bubbles (10 shades).
Reassigning or merging speakers is out of scope for M2.

`diarize.rs` resolves the configured model artifacts, then produces ordered
`SpeakerTurn`s from raw 16 kHz samples. Since WP-62, the production route is
owned in Rust: `ort` v1.16.3 dynamically loads the packaged ONNX Runtime 1.17.1
dylib and runs the downloaded pyannote segmentation model directly. Rust
expands the model's powerset output to local-speaker activity, requests the
existing sherpa embedding extractor for active local windows, and assigns those
embeddings with deterministic incremental-centroid threshold clustering.
`sherpa-rs::diarize::Diarize` and its vendored fast-clustering implementation
are not on the production path. Fixed speaker-count control remains reserved
for WP-49; WP-62 deliberately uses automatic threshold clustering only.

The ONNX Runtime dylib is not a new bundled native dependency: WP-60 already
stages `libonnxruntime.1.17.1.dylib` into `Contents/Frameworks` and the direct
binding resolves that same signed artifact from either the executable directory
or the framework directory. It configures that dylib and creates one ONNX
Runtime environment per worker process before opening segmentation sessions.
Segmentation advances its 160,000-sample inference window by the model's
documented 10% stride; `receptive_field_shift` is used separately to timestamp
output frames. The implementation pins `ort` v1.16.3 because its declared Rust
1.70 minimum is compatible with this project's Rust 1.80 toolchain;
the current `ort` 2.x line requires a newer compiler.

Route-A quality is measured rather than inferred. The ordered 0.95 → 0.75
sweep over the two known-two-speaker reference recordings completed without a
native abort: the 861.57-second recording reaches two clusters at 0.85 and
0.80, while 0.75 produces three; the 92.47-second recording still merges to one
cluster throughout the approved range. The user selected 0.85 as the accepted
automatic clustering threshold; it reaches the long recording's known count
without the 0.75 over-clustering outcome.
The full evidence and metric definition are in
`plans/2026-07-27-wp-62-clustering-feasibility-spike.md`.

### Diarization Process Isolation (WP-53, `diarize_process.rs`)

A fatal signal is not a catchable Rust `Err` or panic. Although WP-62 removes
the known vendored fast-clustering abort from the production route, inference
and embedding extraction still execute native ONNX code. The engine call
therefore remains in a **child
process**: `transcribe_meeting` calls `diarize_process::diarize_isolated`,
which re-executes this same binary with a hidden argv flag
(`--wp-diarize-worker`) rather than shipping a separate sidecar, so there
stays one binary and one code signature. The child runs
`diarize::diarize_samples_with_progress`; the no-progress `diarize_samples`
wrapper is unchanged and retained for `tests/diarize_integration.rs`. Nothing
about the IPC contract, the UI, or the stored schema changes.

The parent classifies four distinct child outcomes — clean exit with a
readable payload, exit by signal (the native crash), non-zero exit (a real
engine error such as a missing asset), and an inactivity kill. They stay
separate rather than collapsing into one error because the embedding-model
fallback (`diarize_with_fallback`, WP-57) retries a crash exactly once with
the other model but must not retry a timeout.
`ChildOutcome::into_result` maps every failure onto the existing
`AppError::Diarization` fail-open path, so an unretryable failure degrades to
speaker-less segments plus a `diarization_warning`, exactly as an ordinary
engine error already did. A successful fallback returns speakers with a
distinct `diarization_warning` naming which model was used instead; no column
records model provenance after the run completes.

Three supervision details are load-bearing. Samples (~55MB for the longest
test recording) cross as a raw `f32` file under `<app-support>/cache/diarize`
rather than a pipe, which would need concurrent write-and-read handling to
avoid filling the pipe buffer. The child is killed on an **inactivity**
budget rather than a total one, driven by progress reported during direct
segmentation batches and embedding extraction. And the child watches
its stdin for EOF: when the app quits mid-run the pipe closes and the worker
stops, instead of being reparented and left holding the models; transport
files older than six hours are swept on the next run to cover a parent that
was killed outright.

The child's dylib search path is still set explicitly
(`DYLD_FALLBACK_LIBRARY_PATH` covering the executable's own directory and the
bundle's `Contents/Frameworks`) rather than inherited by accident from the
launcher. Since WP-60 that is a second line of defence rather than the only
one: the binary carries its own `LC_RPATH` entries, so parent and child both
resolve `@rpath/libonnxruntime` and `@rpath/libsherpa-onnx-c-api` without help
from the environment — which matters because a hardened-runtime build strips
`DYLD_*` unless entitled. See §Build Notes for how the dylibs reach the bundle.

`diarize.rs` also (WP-7) has the turn↔segment merge algorithm:
`merge_segments_with_turns` assigns each segment span the speaker whose turns
maximally overlap it, deterministically tie-broken (lowest speaker id) and
falling back to the nearest turn for a segment in an uncovered gap. `Segment`
carries `speaker_id: Option<i32>` (WP-8, omitted from the JSON when `None` so
existing consumers see no shape change), flowing through
`TranscriptResult`/IPC and `ipc.ts`'s `Segment` interface.

Because whisper's own segmentation is not speaker-aware, one continuous turn
routinely comes back from `transcribe.rs` as many short (~2-3s) fragments that
all land on the same `speaker_id`. `meetings.rs`'s `to_dto` (WP-48) coalesces
consecutive segments sharing the same present `speaker_id` into one display
block — text joined, spanning the first segment's start to the last segment's
end — as long as the gap between them stays within a small tolerance (a
longer gap still starts a new block, since that reads as a real pause).
Segments with `speaker_id: None` are never coalesced with each other or a
neighboring speaker, so a diarization failure never fabricates false turn
continuity. This runs on every read path (`open_meeting`, `save_transcript`,
`rename_meeting`, `set_meeting_source`, `create_empty_meeting`); the
`segments` table itself keeps storing whisper's original fine-grained rows
(see Meeting Model & Persistence above) — coalescing is display-only.

**Diarization now runs automatically as part of `transcribe_file`** (WP-31):
audio is decoded once and both transcription and diarization run over the
same samples (each on its own `spawn_blocking`, off the async reactor);
`diarize_samples` auto-detects the speaker count (no override parameter
exposed yet) and, on success, `assign_speaker_ids` writes each segment's
`speaker_id` in place. Diarization failure of any kind — missing models, an
engine error, or the blocking task itself panicking — is fail-open: it is
logged and the transcription still returns its (speaker-less) segments,
never failing `transcribe_file`. There is still no Stop control, no progress event
spanning both phases, and no persisted meeting entity (those remain the
not-yet-built M2 library epic, F004/WP-11).

**The active diarization embedding model is user-selectable** (WP-52): Settings
offers a three-way choice — None (skip diarization), CAM++ (3D-Speaker,
original default), or TitaNet-large (NeMo, "Recommended" — a stronger
embedding aimed at the over-clustering seen in practice with CAM++ on some
recordings) — persisted as `active_model.diarization` and read fresh on every
`transcribe_meeting` run, no restart needed. `resolve_diarization_models`
picks the active embedding asset by id, not file extension, since the
diarization catalog entry now bundles more than one `.onnx` embedding. A
missing/corrupt active model still fails open to plain segments as above, but
this is no longer silent: `transcribe_meeting` returns a
`{ meeting, diarization_warning }` wrapper (IPC contract below), and the
frontend shows a blocking modal so the degradation is visible rather than only
logged server-side.

**The transcript is persisted before diarization starts** (WP-54):
`transcribe_meeting` decodes and transcribes, saves the transcript, and only
then awaits the diarization pass, which writes speaker ids back as a second
save. The ordering is load-bearing rather than incidental — diarization runs
native sherpa-onnx code that can abort the process it runs in (see the
clustering crash risk and Diarization Process Isolation above), and such an
abort is invisible to
`apply_diarization_outcome`'s fail-open contract, so anything unpersisted at
that moment is lost outright. Saving first bounds the cost of *any* diarization
failure to the speaker labels: the whisper pass survives. The diarization pass
is therefore built as a deferred future, so nothing in it — including the
`transcription_phase` event — can run before the transcript is safe. Two
consequences follow: the meeting's stored status reads `finished` while
diarization is still in flight (the front end shows its transient "Diarizing"
activity instead, so this is not user-visible mid-run), and a failure of the
second, speaker-id save degrades to the same non-fatal warning as any other
diarization failure rather than failing an already-persisted transcription.

The transcript renders segments with real per-speaker coloring (WP-9):
`src/speakerColors.ts` maps a `speaker_id` to one of 10 categorical colors
(`--wp-speaker-0`..`9` in `tokens.css`, dark-mode-aware) and a default
"Speaker N" label; a segment without `speaker_id` renders a neutral bar and
no label rather than a fabricated one. The user can rename any speaker's
label (WP-10, `src/SpeakerLabelEditor.tsx`): the rename applies to every
segment sharing that `speaker_id` and is written into the saved transcript
text ("Label: text" per line). Renames are session-scoped state — reset
whenever a new file is loaded or the current one is removed, so a rename
never leaks into an unrelated transcript — with no persistence across app
restarts (that requires the meeting entity, F004/WP-11, not yet built). This
completes epic WP-1 (M2 speaker-attributed transcription).

## Streaming Audio Capture (WP-68/WP-70, `streaming_audio.rs`) — in progress

Streaming (ADR-014) is a second, separate capture mode from Meeting: near-
real-time transcription of live audio rather than a finished file. This
section covers only its capture/mixing layer (WP-70); the rolling-window
decode pipeline that consumes this module's output is WP-71, not yet built.

`streaming_audio.rs` hands its consumer a continuous, unbounded stream of
16 kHz mono f32 chunks (`crate::audio::SAMPLE_RATE`) via a plain
`std::sync::mpsc::Sender`/`Receiver` pair — mixing granularity (`MIX_TICK`,
100ms) is independent of WP-71's own 5-10s decode window. Its pure logic
(`downmix_to_mono`, `resample_linear`, `mix_mono`) is unit-tested; the two
platform capture sources are:

- **Microphone (`cpal`, macOS-only target dependency)** — implemented. Opens
  the default input device, downmixes to mono and resamples to 16 kHz per
  callback using the pure functions above.
- **System-audio loopback (ScreenCaptureKit)** — **not yet implemented**.
  `SystemAudioCapture::start` always returns an error today, so every session
  currently runs mic-only via the mic-only degradation path (WP-68 decision:
  a single-source capture failure degrades rather than fails the session).
  Two approaches were evaluated and neither reached a verifiable
  implementation in this environment:
  - the ergonomic `screencapturekit` crate compiles, but its mandatory
    `apple-metal` dependency needs Swift compatibility libraries that need a
    full Xcode.app install to link — a dylib/toolchain bundling problem of
    the same shape as WP-60's sherpa-onnx/onnxruntime work, not a quick fix;
  - the raw `objc2-screen-capture-kit` binding avoids Swift entirely (same
    Objective-C-runtime approach already used elsewhere in this dependency
    tree, e.g. `objc2-app-kit` via Tauri/muda), but correctly extracting PCM
    audio out of a `CMSampleBuffer` requires calling CoreMedia's raw C
    audio-buffer-list API, whose exact behavior could not be confirmed
    without running real captured audio through it.

Mutual exclusion with an active Meeting transcription (both would share the
one cached Whisper `AppState` context) is WP-71's concern, not implemented
here — this module only captures and mixes audio, it does not decode it.

## Structured Notes (M3, `notes.rs`) — planned

llama.cpp running quantized Qwen2.5-Instruct on Metal generates **structured
meeting notes** in Russian from the transcript: summary, key decisions, action
items (owner + task), open questions, participants. Generation is **manual**
(the **Create MFU** button, enabled only after transcription finishes) and
**UI-blocking**; the result is editable (auto-saved), copyable, and clearable.

## Settings & Model Management (`settings.rs`, `models.rs`) — M2 beta, M3 release

Settings live in a small **key–value store** in the app support directory
(theme, `ui_language`, and each task's active model), applied immediately and
across restarts. The React layer owns **theming** (light / dark / system, plus
release themes) and **i18n** (English default, release languages); the OS scheme
drives the *System* theme.

`models.rs` manages a **fixed, app-defined catalog** of the model(s) each task
needs (transcription = Whisper, diarization = sherpa-onnx segmentation +
selectable embedding, notes = llama/Qwen at M3). **Download** fetches from a
known URL, streams progress, and marks a model ready only after **SHA
verification**; **Delete** removes the local file. A task whose required model
is absent is disabled or degrades (Transcribe needs the Whisper model;
diarization degrades per F002-R7). Beta manages **one model per task** for
transcription; at release other tasks may hold several with an **Active**
selection. Diarization is ahead of that general timeline (WP-52): its catalog
entry already holds one shared segmentation asset plus multiple
independently-downloadable embedding variants (CAM++, TitaNet-large), addressed
by a synthetic `"diarization-<variant>"` id, with an `active_model.diarization`
setting selecting which embedding is Active (or `"none"` to skip diarization
entirely — the default for every user, including those upgrading from before
this selection existed). This supersedes the earlier "manual model placement /
deferred model management" note.

## Export (`export.rs`)

A meeting renders to **Markdown** or **plain text** (transcript and/or notes),
written to a user-chosen destination. Copy-to-clipboard reuses the same
rendering (the header's meeting-label **copy** copies the transcript).

## IPC Contract

| Command | Purpose | Milestone |
|---|---|---|
| `open_file_dialog` | Pick a source audio/video file | M1 |
| `create_meeting()` | Create an empty meeting; returns its id | M2 |
| `attach_file(meeting, path)` | Attach the source file to a meeting | M2 |
| `create_transcription(meeting, model)` | Transcribe the attached file into the meeting; emits progress. No language argument — it is always detected (ADR-012) | M2 |
| `cancel_transcription(meeting)` | Abort a running transcription (Stop) | M2 |
| `list_meetings()` | Meetings list (summaries) | M2 |
| `open_meeting(id)` | Full meeting (segments, notes, meta) | M2 |
| `rename_meeting(id, title)` / `delete_meeting(id)` | Library management | M2 |
| `update_segment(meeting, seg, text)` / `update_notes(meeting, notes)` | Auto-saved edits | M2/M3 |
| `export_meeting(id, format, target)` | Write Markdown / plain text | M2 |
| `list_models()` | Available (downloaded) Whisper models for the switcher | M2 |
| `diarize_meeting(id)` | Produce + merge speaker turns | M2 |
| `get_settings()` / `set_setting(key, value)` | Read/update settings (theme, ui_language, active model) | M2 |
| `list_task_models()` | Per-task model catalog with download state | M2 |
| `download_model(id)` / `delete_model(id)` | Fetch (SHA-verified, progress) / remove a model | M2 |
| `set_active_model(task, id)` | Choose the active model for a task | M3 |
| `check_update()` / `apply_update()` | App update | M3 |
| `generate_notes(id)` | Generate structured MFU notes (Create MFU) | M3 |

Events: `transcription_progress { id, fraction }`, `transcription_done`,
`transcription_error`,
`model_download_progress { id, fraction, stage }` — where `stage` is
`downloading` while bytes arrive and `verifying` while the fetched file is
SHA-hashed, a pass long enough on a large model that the UI must name it rather
than show a full bar. `Segment` is
the shared transcript unit
(`{ id, start_ms, end_ms, text, speaker_id? }`). Errors are `AppError` serialized
to a human-readable string.

## Security And Privacy

**Transcription and MFU note generation make no network calls** — they run
entirely on-device, and there is **no telemetry**. The **only** networked
operation is **user-initiated model downloads** (and, at release, the app
update), fetched from known URLs and **SHA-verified**. Audio, transcripts, and
notes never leave the device. File writes: the temporary ffmpeg WAV (deleted
after use), the SQLite library and the settings store under the app support
directory, downloaded model files, and user-chosen export destinations. No
`.env` or secret handling.

## Build Notes

- Tauri v2 + React 19 + TypeScript; Vite dev server on port 1420.
- `whisper-rs = { features = ["metal"] }`; `rusqlite = { features = ["bundled"] }`.
- ffmpeg on PATH. M2 adds sherpa-onnx (via the `sherpa-rs` crate, prebuilt
  binaries fetched at build time); M3 adds llama.cpp — both Metal, local.
- Streaming (WP-70) adds `cpal` for microphone capture, macOS-only-target
  dependency like `whisper-rs`'s `metal` feature — its default Linux backend
  (`alsa-sys`) needs ALSA dev headers CI does not install. System-audio
  loopback has no dependency yet (not implemented — see Streaming Audio
  Capture above).
- M2 adds an HTTP client for SHA-verified model downloads and a settings store;
  front-end gains theming (light/dark/system) and i18n (English default).
- Native dylib packaging (WP-60): `sherpa-rs-sys` leaves
  `libsherpa-onnx-c-api.dylib` and `libonnxruntime.<version>.dylib` in the cargo
  profile directory, and the linker records them as `@rpath/…`. `build.rs`
  therefore does two things on macOS — links every binary with
  `@executable_path/../Frameworks` and `@executable_path` rpaths, and stages
  both dylibs into the generated `src-tauri/frameworks/` for
  `bundle.macOS.frameworks` to copy into `Contents/Frameworks`. Without both, a
  packaged build aborts at dyld before `main` while `cargo run` keeps working,
  because cargo supplies a fallback search path the `.app` never gets.
  `src-tauri/tests/packaging.rs` asserts the config, staging, and rpaths.
  Signing and notarizing the bundled dylibs is not solved.
- Run: `npm install`, then `npm run tauri:dev`.

## Ownership

| Concern | Owner |
|---|---|
| Command/event registration, app state, model cache | `src-tauri/src/lib.rs` |
| Audio normalize + decode | `src-tauri/src/audio.rs` |
| Whisper transcription + progress | `src-tauri/src/transcribe.rs` |
| SQLite meeting library | `src-tauri/src/store.rs` |
| Export rendering | `src-tauri/src/export.rs` |
| Error type | `src-tauri/src/error.rs` |
| Two-pane shell: meetings list, meeting workspace, editors | `src/` |
