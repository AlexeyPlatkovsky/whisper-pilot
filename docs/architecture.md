# WhisperPilot Architecture

Technical architecture for the offline meeting-mfu workspace. Product scope is
owned by `docs/idea.md`; UX by `docs/design.md`. This document owns the layer
map, pipeline, meeting/data model, IPC contract, models, and build MFU. The
core unit is a **Meeting** (one transcription of one source file).

## Layer Map

```
React UI (src/)  ──Tauri IPC──▶  Rust core (src-tauri/src/)
  meetings list                    lib.rs        crate root; `run()` registration
  meeting workspace                commands/     thin Tauri command layer (per-domain modules)
  transcript editor                audio.rs      ffmpeg normalize + WAV decode
  MFU panel                        transcribe.rs whisper (Metal) decode; Streaming progress callback
  settings screen                  store.rs      SQLite meeting library (meetings, segments, MFU)
  ipc.ts / events                  meetings/     meeting persistence facade (`dto.rs` = DTOs/coalesce)
  theming / i18n                   state.rs      AppState (model cache, running-run slots)
  [WP-68] StreamingView.tsx        error.rs      AppError → serialized to JS
                                   settings.rs   key–value settings store (theme, ui_language, active models)
                                   models/       model catalog (`catalog.rs`) + download (`download.rs`)
                                   [M2] diarize/   sherpa-onnx speaker turns + merge (clustering/segmentation/speakers/pipeline)
                                   [M3] mfu.rs     llama.cpp structured meeting MFU
                                   [WP-68] streaming_audio.rs  mic + system-audio capture/mix (see below)
                                   [WP-68] streaming_session.rs  rolling-window decode + mutual exclusion
                                   [WP-68] streaming_store.rs  SQLite streaming_sessions/streaming_segments
                                   [WP-68] streaming.rs  Streaming IPC facade (list/open/rename/delete)
```

The Rust core does all heavy work and owns persistence; the React layer is a
two-pane shell — a meetings list plus the active meeting's workspace. Blocking,
CPU/GPU-heavy work (model load,
transcription, diarization, detail generation) runs on `tokio::task::spawn_blocking`
so IPC and the UI stay responsive. Long operations report progress; Streaming
capture has its own stop command, while Meeting transcription runs to completion.

## Meeting Model & Persistence (`store.rs`)

The library is a local **SQLite** database (`whisperpilot.sqlite3` in the app
support directory, via bundled `rusqlite`). WP-16 implements the idempotent
schema and Rust CRUD store. WP-21/WP-22 expose create/list/open/rename/delete
commands and hydrate the workspace from persisted meetings. A **meeting** is
one transcription of one source file. Meetings
**reference the original file path** — audio is not copied — so a meeting whose
source has moved or been deleted is readable but cannot be re-transcribed (a
defined "source missing" state). `MeetingDto.source_missing` (WP-23) is
computed fresh on every DTO build — `to_dto` in `meetings/dto.rs` checks
`Path::exists()` on the stored `source_path` — rather than stored, so it
always reflects the file's current state instead of a snapshot from whenever
it was last attached or transcribed. The front end disables **Transcribe**
and shows an explanatory detail when set; the transcript and MFU stay
readable and editable either way.

Entities (indicative):

| Entity     | Key fields                                                                        |
| ---------- | --------------------------------------------------------------------------------- |
| `meetings` | id, title, source_path, source_name, created_at_ms, duration_ms, language, status |
| `segments` | meeting_id + ordinal (composite key), start_ms, end_ms, text, speaker_id (M2)     |
| `MFU`    | meeting_id, summary, decisions, action_items, open_questions, participants (M3)   |

The store replaces/reads segments in ordinal order, upserts a single MFU
record per meeting, and cascades meeting deletion to dependent rows. Segment
text and MFU edits are **auto-saved** (WP-17): the front end debounces each
edit (500ms idle) and calls `update_segment`/`update_mfu`, which persist to
the DB immediately; there is no explicit save state or button. Export is a
separate, explicit write to an external file.

Stored `segments` rows start as whisper's original fine-grained spans —
`to_dto` (`meetings/dto.rs`, WP-48) coalesces consecutive same-speaker rows into
larger display blocks on every read path (see Speaker Diarization below), and
that is what the UI renders and edits as one block. `update_segment` (WP-17)
therefore writes back at the same granularity the user edited: it re-derives
the current coalesced list, replaces the edited block's text, and rewrites
_all_ segment rows for the meeting to match it. This means editing any one
coalesced block also collapses the other, untouched blocks in that meeting to
one row per display block from that point on — the fine-grained pre-edit
ordinals are not preserved once an auto-save has happened. Speaker labels
(the user-facing rename in `SpeakerLabelEditor`) remain session-only, not
persisted; only segment text and MFU are in WP-17's scope.

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
`meetings.language` an _output_ of a run rather than an input to one. Detection
uses the first 30 seconds of audio, so a recording that opens with silence can
misdetect — a known limitation. Meeting output is timestamped
`Segment`s persisted to the meeting.

Forcing a language is deliberately unreachable rather than merely defaulted:
decoding audio as a language it is not in makes Whisper emit one hallucinated
line per 30-second window instead of the transcript. `DecodeSettings` names the
configuration as plain data so it can be asserted in a unit test — notably
`detect_language_only`, which must stay `false` because whisper.cpp returns
immediately after detection when it is set, yielding an empty transcript.

Meeting does not install a Whisper progress or abort callback: the bundled
Metal path is kept identical to the known-good main-branch decode (WP-86).
Streaming retains its separate state-reuse path. A percent-progress event is
therefore not emitted during Meeting decode; the UI uses its indeterminate status.

On macOS, `llama-cpp-2` is dynamically linked while Whisper remains static.
Both engines embed incompatible ggml versions with the same exported symbol
names; linking both statically can resolve Whisper's Metal calls to llama.cpp's
ggml implementation and corrupt encoder output. The llama/ggml dylibs are
staged into the app bundle alongside the sherpa-onnx native libraries.

The **Transcribe** run is a two-phase pipeline: transcription, then **diarization

- merge** (M2, `diarize/`). The transcript is persisted between the two phases,
  so the meeting is already marked finished while diarization is still running (see
  Speaker Diarization below). A progress spinner spans both phases; Meeting
  transcription runs to completion and the isolated worker design for restoring
  Stop is tracked separately (WP-87). If diarization is unavailable or fails,
  the run still finishes with plain (speaker-less) segments. Re-running
  Transcribe on a meeting that already has
  a transcript replaces it (and any MFU) after a confirmation.

The model is the `large-v3-turbo` artifact downloaded and SHA-verified via the
Settings AI models section (F005, `models/`) into the app support directory;
override the path for development with `WHISPERPILOT_MODEL_PATH`.

## Speaker Diarization (M2, `diarize/`)

sherpa-onnx segmentation + embedding models produce speaker turns, merged onto
segments by time overlap to set each segment's `speaker_id`. Speaker count is
auto-detected with an optional override. Labels are generic ("Speaker N",
English per ADR-011) and user-renamed within the current session. The
transcript renders as a per-speaker chat of colored bubbles (10 shades).
Reassigning or merging speakers is out of scope for M2.

**Standalone "Diarize" action** (`diarize_meeting` command, Transcript
header, next to the Editable indicator): re-runs speaker identification
alone on an already-transcribed meeting, without re-transcribing. Re-decodes
the source audio (samples are never persisted) and re-runs the same
`diarize_process::diarize_with_fallback` pass `transcribe_meeting` uses
internally, then assigns the resulting turns onto the meeting's
already-persisted segments by time overlap
(`meetings::diarize_meeting_segments` → `diarize::assign_speaker_ids`) and
saves the result. Unlike the diarization folded into a Transcribe run, a
failure here is a real error surfaced to the user, not a fail-open warning —
diarization is what was explicitly asked for, so there is no already-safe
transcript to fail open onto. Requires an active diarization model, an
existing transcript, and a readable source file (disabled client-side
otherwise); reuses the same time-overlap assignment as the automatic pass,
so it re-diarizes at whatever segment granularity currently exists —
full per-utterance precision on a never-diarized meeting, or the coarser,
already-coalesced per-speaker blocks on one diarized before.

`diarize/` resolves the configured model artifacts, then produces ordered
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

### Diarization Process Isolation (WP-53, `diarize_process/`)

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
`DYLD_*` unless entitled. See §Build MFU for how the dylibs reach the bundle.

`diarize/` also (WP-7) has the turn↔segment merge algorithm:
`merge_segments_with_turns` assigns each segment span the speaker whose turns
maximally overlap it, deterministically tie-broken (lowest speaker id) and
falling back to the nearest turn for a segment in an uncovered gap. `Segment`
carries `speaker_id: Option<i32>` (WP-8, omitted from the JSON when `None` so
existing consumers see no shape change), flowing through
`TranscriptResult`/IPC and `ipc.ts`'s `Segment` interface.

Because whisper's own segmentation is not speaker-aware, one continuous turn
routinely comes back from `transcribe.rs` as many short (~2-3s) fragments that
all land on the same `speaker_id`. `meetings/dto.rs`'s `to_dto` (WP-48) coalesces
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
never failing `transcribe_file`. Meeting transcription has no in-process Stop;
the isolated worker design for restoring it is tracked separately (WP-87).

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
that moment is lost outright. Saving first bounds the cost of _any_ diarization
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

## Streaming Audio Capture (WP-68/WP-70, `streaming_audio.rs`)

Streaming (ADR-014) is a second, separate capture mode from Meeting: near-
real-time transcription of live audio rather than a finished file. This
section covers only its capture/mixing layer (WP-70); the rolling-window
decode pipeline that consumes this module's output is WP-71 (see below).

Both capture sources need macOS permissions this app never previously
required: `src-tauri/Info.plist` declares `NSMicrophoneUsageDescription`
(mic, via `cpal`) and `NSScreenCaptureUsageDescription` (system-audio
loopback, via `screencapturekit`), auto-merged into the bundle by Tauri
(same directory as `tauri.conf.json`). The `NSMicrophoneUsageDescription`
key is not optional UX polish — without it, macOS crashes the app outright
the first time it requests microphone access.

`streaming_audio.rs` hands its consumer a continuous, unbounded stream of
16 kHz mono f32 chunks (`crate::audio::SAMPLE_RATE`) via a plain
`std::sync::mpsc::Sender`/`Receiver` pair — mixing granularity (`MIX_TICK`,
100ms) is independent of WP-71's own 5-10s decode window. A background mixer
thread drains whichever source buffer(s) are active every tick and sums+
clamps them (`mix_mono`) to `[-1.0, 1.0]`, or passes one source through
unmixed if only one is active. Pure logic (`downmix_to_mono`,
`resample_linear`, `mix_mono`) is unit-tested (12 tests); the two platform
capture sources, both macOS-only target dependencies:

- **Microphone (`cpal`)** — opens the default input device, downmixes to
  mono and resamples to 16 kHz per callback using the pure functions above
  (the device's native rate/channel count vary; ScreenCaptureKit does not
  need this step, see below).
- **System-audio loopback (`screencapturekit` crate)** — requests 16 kHz
  mono directly from `SCStreamConfiguration` (a natively supported
  rate/channel-count pair), so its `SCStreamOutputTrait::did_output_sample_buffer`
  callback reinterprets `AudioBufferList`'s raw bytes as little-endian f32
  with no resampling needed. `screencapturekit`'s mandatory `apple-metal`
  dependency links `libswift_Concurrency.dylib`, an OS-provided Swift
  runtime library that exists only in the dyld shared cache (no standalone
  file, unlike the sherpa-onnx/onnxruntime dylibs WP-60 bundles) — `build.rs`
  adds a fixed `-Wl,-rpath,/usr/lib/swift` link arg so the binary resolves it
  without bundling, and `tests/packaging.rs`'s WP-60 regression test carries
  an explicit `OS_PROVIDED_DYLIBS` exemption (plus its own rpath assertion)
  for this dylib rather than treating it as one to bundle. Building this also
  requires the full Xcode.app (not just Command Line Tools), for the Swift
  compatibility libraries `apple-metal` needs at link time.

Mic-only degradation (WP-68 decision: a single-source capture failure
degrades rather than fails the session) means either source failing to start
— e.g. system-audio permission denied — falls back to whichever source(s)
remain; only both failing is a hard error. This path is architecturally real
but not runtime-verified: granting real microphone/screen-recording
permissions and running the packaged `.app` is outside what this environment
can do, so the OS-level capture wrappers are compiled, linked, and reviewed,
not exercised end-to-end.

Mutual exclusion with an active Meeting transcription is WP-71's concern, not
implemented here — this module only captures and mixes audio, it does not
decode it.

## Streaming Decode/Session Pipeline (WP-68/WP-71, `streaming_session.rs`)

Decodes the continuous sample stream `streaming_audio.rs` produces on fixed,
non-overlapping ~7s windows (`WINDOW_SECONDS`, the midpoint of WP-68's
approved 5-10s latency budget). One session decodes every window through a
single `WhisperState` (WP-82): `run_windowed_decode` builds one
`WhisperSessionDecoder` when the loop starts and reuses it via
`transcribe::transcribe_with_state`, because each state owns a full GPU
backend plus its KV/compute buffers — one state per window was one backend
init/free cycle per window. State reuse across calls is upstream's own
`whisper_full` pattern (each call clears results, recomputes the mel, and
clears the self-attention KV cache); Meeting keeps one state per whole-file
run. Each window gets its own language detection, unlike Meeting's
once-per-file detection (ADR-012), since a live session has no single fixed
language the way a finished file does. A word can split across a window
boundary — an accepted, documented trade-off for non-overlapping windows,
not a silent one.

**Fail-open per window** (mirroring diarization, ADR-013): a window whose
decode errors is skipped — logged, no text emitted for that span — rather
than ending the session. `WindowResult.outcome` carries the `Result` through
rather than the loop propagating it.

**Mutual exclusion** (`WhisperUsageGuard`, backed by a new `AppState.
whisper_busy: AtomicU8`): a Meeting transcription and a Streaming session
cannot run concurrently. Both use the one cached Metal context, with a fresh
Whisper state for each whole-file Meeting run and a reused state for each
Streaming session. Concurrent native model work remains intentionally
serialized. `transcribe_meeting` acquires this guard for its whole duration,
released on drop;
Streaming's own session start will do the same once wired to IPC. This
guard is deliberately not narrowed to "only block the _other_ kind" — two
concurrent Meeting transcriptions are serialized by the same guard, a small,
disclosed safety tightening beyond WP-68's literal Streaming-vs-Meeting
ask, since the underlying safety property (one decode at a time against the
shared context) does not depend on which caller is asking.

Wired to Tauri IPC via `start_streaming_session`/`stop_streaming_session` in
`commands/streaming.rs` (WP-73) — see the Streaming Runtime & UI section below for the
command/event layer that ties this module to `streaming_audio.rs` and
`streaming_store.rs`.

**Latency is not measured against real hardware.** The feasibility spike
WP-68's own DoD requires — measuring real per-window decode latency across
supported Mac hardware — needs a person running this with a downloaded
model, which this environment cannot do. `WindowResult.decode_ms` exists so
that measurement is possible once someone can run it; `WINDOW_SECONDS`
itself is not yet the finalized, measured threshold.

## Streaming Persistence (WP-68/WP-72, `streaming_store.rs`)

A `StreamingSession` is a separate entity from a `Meeting` (WP-68 D5): no
backing file, a possibly-mixed per-window language rather than one per-file
language, and no diarization — none of which fit the `meetings` table's
shape. `streaming_store.rs` opens its own `Connection` to the same
`whisperpilot.sqlite3` file `store.rs` uses (SQLite supports multiple
connections to one file; `store::shared_database_path` is the one shared
path constant) and owns two new tables, `streaming_sessions` and
`streaming_segments`, parallel to but independent of `meetings`/`segments`.

**Incremental save, not replace-wholesale.** Unlike `Store::replace_segments`
(delete-all-then-reinsert, appropriate for a finished file decoded once),
`StreamingStore::append_window` upserts one window at a time as
`streaming_session.rs`'s decode loop produces `WindowResult`s — an
`ON CONFLICT(session_id, window_index) DO UPDATE` makes a retried save
idempotent. This is what makes WP-68's crash-recovery DoD true: only the
last in-flight window can be lost, because every prior window is already
committed by the time the next one starts decoding. Each append also
advances `streaming_sessions.updated_at_ms` in the same transaction, so a
session that stalls (capture keeps running but decode stops producing
windows) is distinguishable from one making progress.

**A failed window is stored, not dropped.** `NewStreamingWindow.outcome_ok`
records whether that window's decode succeeded (per `streaming_session.rs`'s
fail-open contract) — a failed window still gets a row (empty text,
`outcome_ok = false`) rather than being skipped entirely, so replaying a
session's transcript can render "this span failed to decode" instead of
silently reading as a span with no speech at all.

`streaming.rs` is the IPC-facing facade over this store (WP-73), mirroring
`meetings/`'s "open the store fresh per call" convention: `list_
streaming_sessions`, `open_streaming_session`, `rename_streaming_session`,
`delete_streaming_session`, `create_streaming_session`. Its DTOs
(`StreamingSessionDto`, `StreamingWindowDto`) are the JSON shape the
Streaming tab consumes.

`streaming_translations` (WP-92) is the newest table in this store, keyed by
`(session_id, paragraph_key, target_language)` with `ON DELETE CASCADE` on
`session_id`, created in the same `CREATE TABLE IF NOT EXISTS` schema batch as
`streaming_prettified`. `paragraph_key` is the `window_index` of a paragraph's
first window — the front-end paragraph grouping's only stable anchor this
store can key on, since it has no paragraph concept of its own.
`StreamingStore::upsert_translation` follows `append_window`'s
`ON CONFLICT … DO UPDATE` idiom, so a repeated translate for the same key
overwrites in place rather than duplicating; `list_translations` reads back
every stored translation for one session and target language. Each row also
stores the source paragraph text it was translated from, so
`StreamingTranslation::is_stale(current_source_text)` can tell a caller when a
later window extended the paragraph and the stored translation no longer
matches — re-translation, not display, is then the caller's job. See
Paragraph Translation below for the local-LLM call and command that populate
this table.

## Streaming Runtime & UI (WP-68/WP-73, `start_streaming_session` /

`stop_streaming_session` in `commands/streaming.rs`, `src/StreamingView.tsx`)

Starting a session ties `streaming_audio.rs` (capture), `streaming_
session.rs` (decode/mutual-exclusion), and `streaming_store.rs`
(persistence) together via two new `AppState` fields:
`whisper_busy` (WP-71's guard) and `streaming_runtime: Mutex<Option<
StreamingRuntime>>` (macOS-only — the type doesn't exist on the Linux CI
target), holding the live `streaming_audio::StreamingSession` capture.

`start_streaming_session` claims `whisper_busy`, then either creates a fresh
session row (no `session_id` argument) or **resumes** a previously-stopped
one: given a `session_id`, `streaming::resume_streaming_session` validates
the session exists and is `STOPPED` (rejecting an already-`ACTIVE` one —
resuming it would double-capture), computes the window index to continue
counting from (one past the last persisted window, or 0 if none was ever
saved), and flips its status back to `ACTIVE` via the new `StreamingStore::
mark_active`. Either way it then starts capture and spawns two `spawn_
blocking` tasks: one runs `run_windowed_decode` (now taking a `starting_
window_index` so a resume's window numbering — and thus each window's
`start_ms`, still an offset into this take's audio timeline, not wall-clock
time — continues rather than restarting at 0), the other (`drive_streaming_
results`) consumes its `WindowResult`s — persisting each via `append_window`
and emitting `streaming_window` — until the results channel disconnects, at
which point it marks the session stopped and releases `whisper_busy`.
`src/StreamingView.tsx`'s Start action resumes the currently-open session
when it's a past, stopped one (and relabels itself "Resume" accordingly);
the header's "+"/New icon always starts fresh regardless of what's open,
via a separate `handleStartNew` that never passes a `session_id`.
`stop_streaming_session` only has to do one thing: take `streaming_runtime`
out of `AppState` and let it drop. Dropping the held `streaming_audio::
StreamingSession` stops both capture streams, which cascades through the
mixer thread → sample channel → decode loop → results channel, ending
`drive_streaming_results` on its own. Both `SCStream` (`screencapturekit`)
and `cpal::Stream` are `Send`/`Sync` (the former explicitly, documented in
the crate itself; the latter via its own `assert_stream_send!`), so storing
the capture in `AppState`'s tokio `Mutex` needed no additional unsafe code.

On non-macOS targets, `start_streaming_session`/`stop_streaming_session`
are still registered (same command names, same generated-handler list) but
return a "macOS only" error — keeping the frontend's command surface
identical across platforms rather than branching on `cfg` in TypeScript.

`src/StreamingView.tsx` is a self-contained top-level view (entered/exited
like `SettingsScreen`), not a change to the existing Meeting workspace — but
since this UI redesign it now mirrors `App.tsx`'s window shell wholesale
(`.wp-header`/`.wp-info-bar`/`.wp-sidebar`/`.wp-transcript-panel`/`.wp-mfu`,
plus the shared `ActionIcon` component) rather than keeping its own bespoke
sidebar/action-row markup, so the two windows can't drift apart visually. The
two windows are switched via a `ModeToggle` control in each one's sidebar
(`src/ModeToggle.tsx`, "Meeting" / "Streaming" segments) instead of a
dedicated header icon — clicking the inactive segment calls `onClose` (from
Streaming) or opens Streaming (from Meeting). Because `StreamingView` is a
mounted child rather than inline JSX in `App`, opening Settings from inside
it (its header also carries the same Toggle-sidebar/New/Settings icon group
as Meeting's) renders `SettingsScreen` as a fixed-position `.settings-overlay`
layered on top rather than replacing the tree — swapping to `SettingsScreen`
outright would unmount `StreamingView` and drop a live recording's in-flight
state; the overlay keeps it mounted underneath. `StreamingView` owns: the
session list (rename/delete, styled like Meeting's `wp-meeting-row`, mirroring
Meeting's), Start/Stop/Craft/Copy/Export/Delete in the header's Main Actions
row, a live transcript that appends `streaming_window` events for whichever
session is currently open (`upsertWindow` replaces rather than duplicates a
resent `window_index`, and ignores events for a session that isn't the open
one — stale events from a just-stopped session are possible during the
transition), an Audio Source chip in the info bar for the `streaming_sources`
indicator so mic-only degradation is visible rather than silent, a header
status widget (WP-76) cycling Ready → Starting… → On Air → Crafting
MFU…/MFU Failed (WP-77) → Prettifying…/Prettify Failed (WP-75) (elapsed
timer, `h:mm:ss` past one hour) as `isRunning`/`busy`/`craftingId`/
`craftFailed`/`prettifyingId`/`prettifyFailed` change, driven by a single
`src/streamingStatus.ts` resolver (mirroring `meetingStatus.ts`'s one-table
approach) that both the header widget and the sidebar row's status dot read,
reusing `App.tsx`'s existing `.wp-status`/`.wp-tone--*` pattern; a Craft
button (WP-77, in the header's Main Actions row) that generates structured
MFU into a `.wp-mfu` panel — see Structured MFU above; and a Prettify
button (WP-75, in the transcript panel's own header actions, alongside its
Accept/Cancel/Revert controls) — see Transcript Prettify below. A fail-open
window renders as `[unavailable]`, not blank space, so a decode failure reads
differently from genuine silence — same distinction `outcome_ok` preserves
in storage.

Rename and delete (both the header title's icons and each sidebar row's) go
through in-app `.modal-overlay`/`.modal-panel` dialogs, matching `App.tsx`'s
Meeting rename/delete pattern exactly — not `window.prompt`/`window.confirm`,
which Tauri's WKWebView does not reliably wire up (they silently no-op rather
than showing anything).

The raw live transcript groups windows into `<p>` paragraphs via `src/
paragraphs.ts`'s `groupWindowsIntoParagraphs` — a client-side sentence-
boundary + length heuristic (new paragraph once the accumulated text is both
long enough and ends a sentence, or once too many windows have piled up
without ever hitting a sentence end), since windows carry no pause/VAD signal
of their own to split on (they're fixed-size slices of continuous audio, not
silence-delimited — see Streaming Decode/Session Pipeline above). Each
window keeps its own span (fail-open styling, per-window tooltip) inside its
paragraph, so this is purely a rendering grouping, not a change to the
underlying transcript data. Prettify's LLM cleanup does not yet also emit
paragraph breaks — a possible follow-on, not yet built.

## Structured MFU (M3, `llm.rs`)

llama.cpp running quantized Qwen2.5-Instruct on Metal generates **structured
MFU** from a transcript: summary, key decisions, action items, open
questions, participants — in Russian or English depending on which the
transcript itself is in (Cyrillic-character detection in `llm::build_prompt`).
`llm::generate_mfu` returns a domain-agnostic `GeneratedNotes` (no id field);
each caller attaches its own id before persisting. For Meeting, generation is
manual (the **Create MFU** button, enabled only after transcription finishes)
and UI-blocking; the result is copyable, not separately editable or
clearable. Streaming reuses the same `generate_mfu` call (WP-77,
`generate_streaming_notes`) for a Streaming session's transcript, gated the
same way (enabled only once the session is stopped) and persisted in its own
`streaming_notes` table (`streaming_store.rs`), parallel to but independent
of Meeting's `MFU` table.

## Transcript Prettify (WP-75, `llm.rs`, `src/diff.ts`) — Streaming only

A second, distinct local-LLM use of the same model: `llm::prettify_transcript`
performs conservative cleanup of a Streaming transcript, returning plain
cleaned text (not the structured-MFU JSON template `generate_mfu` uses).
The backend rejects empty, language-dropping, excessively shortened or
expanded candidates and candidates that omit protected numbers or technical
terms, so an unsafe rewrite never reaches the review UI. Unlike Craft/MFU,
Prettify's result is **not persisted on generation** —
`generate_streaming_prettify` only returns a validated candidate for review.
The frontend diffs it client-side against the original transcript
(`src/diff.ts`'s `computeWordDiff`, a self-contained LCS word-diff — no new
dependency) and renders it as `<del>`/`<ins>` spans with Accept/Cancel
controls. `accept_streaming_prettify` persists accepted text to its own
`streaming_prettified` table (`streaming_store.rs`, same `session_id`-keyed
upsert/cascade-delete shape as `streaming_notes`); the `Revert Prettify`
control calls `revert_streaming_prettify` to delete that row and restore the
raw per-window transcript for display/copy/export. The two LLM-generation
commands reuse `streaming::build_streaming_transcript`'s guards (session
exists, stopped, non-empty transcript); Accept and Revert operate on an
existing session row. Craft and Prettify are kept from overlapping only by
the front end disabling their buttons while either is in flight — there is
no backend guard between those two commands, so nothing in the core itself
would stop two concurrent requests from both reaching the shared model. (See
Paragraph Translation below for the first real backend guard against this
class of contention, though it currently covers only translation.)

## Paragraph Translation (WP-92 core/persistence, WP-93 UI; `llm.rs`, `src/StreamingView.tsx`)

A third local-LLM use of the same shared model, alongside Craft MFU and
Prettify: `llm::translate_paragraph` translates one Streaming paragraph into
a target language (`"en"` or `"ru"`) via the same llama.cpp completion path,
using its own prompt (`build_translate_prompt`, branching on the target
language) and its own candidate validation
(`validate_translation_candidate`) — rejecting an empty or whitespace-only
result, a result disproportionately shorter or longer than the source, and a
result still predominantly in the source script when the target script
differs. `ensure_translation_fits_context_budget` rejects (rather than
truncates) a paragraph whose estimated token count would overflow `CTX_SIZE`,
using a script-aware chars-per-token estimate — Cyrillic tokenizes denser
than Latin under Qwen/ChatML-style tokenizers, so the same character count
budgets fewer tokens for Cyrillic text.

Unlike Craft and Prettify (previous section), translation has its own real
backend concurrency guard: `AppState::translation_busy` (an `AtomicBool`,
deliberately independent of `whisper_busy` so translation never blocks or is
blocked by the streaming decode loop) and `llm::TranslationUsageGuard`, a
non-blocking single-flight RAII guard mirroring `streaming_session::
WhisperUsageGuard`'s claim/release idiom. Contention returns
`AppError::TranslationBusy`, a distinct, UI-retryable error, rather than
queuing or blocking. This guard only single-flights translation against
itself — it does not also cover Craft or Prettify.

The `translate_streaming_paragraph(session_id, paragraph_key, target_language,
text)` command (`commands/mfu.rs`) validates the request cheaply first
(`streaming::ensure_translation_request_is_valid`: supported target language,
session exists, non-empty source text) before resolving the active LLM model
path or acquiring the single-flight guard, then runs `streaming::
translate_and_store` — which calls the model and upserts the result into
`streaming_translations` (see Streaming Persistence above) — on a
`spawn_blocking` task, and returns the translated text. `src/ipc.ts` exposes
this as a typed `translateStreamingParagraph` wrapper with a
`StreamingTranslationTargetLanguage` type.

Its read counterpart, `list_streaming_translations(session_id,
target_language)` (`commands/streaming.rs` → `streaming::
list_streaming_translations`), validates the target language and that the
session exists, then returns every persisted `{paragraph_key, source_text,
translated_text}` row for that session and target language via
`StreamingStore::list_translations`. `src/ipc.ts` exposes it as
`listStreamingTranslations`, typed `StreamingTranslationRow[]`. This closed
the gap the previous revision of this section left open: WP-92 persisted
translations but shipped no way to read them back.

**WP-93 (front-end, `src/StreamingView.tsx`):** the transcript header gains
a Live Translation control between the title group and the action cluster —
switch + target-language select (English/Русский) — and, while it is on,
the transcript content renders as a two-column paired-row grid instead of
`groupWindowsIntoParagraphs`'s usual single-column flow. See `docs/design.md`
("Center — transcript") for the full layout, row states, and header-control
rules. Behaviorally: a paragraph is enqueued for translation when it closes
(a later paragraph exists, or capture stopped); turning the switch on
backfills the whole session oldest-first; exactly one
`translateStreamingParagraph` call is in flight at a time, and the queue
never delays rendering of incoming windows. Before queuing anything, an
effect calls `listStreamingTranslations` so a row already persisted for this
session and target language is reused without a model call; a persisted row
whose `source_text` no longer matches the paragraph's current text is stale
and gets re-queued instead. A paragraph whose windows are all already in the
target language is never sent to the model — its right cell mirrors the
original text in a muted style. Switching the toggle off mid-queue, or
switching or deleting the session, cancels pending work and discards any
in-flight result a subsequent change has superseded.

## Settings & Model Management (`settings.rs`, `models/`) — M2 beta, M3 release

Settings live in a small **key–value store** in the app support directory
(theme, `ui_language`, each task's active model, export file type, the
WP-88 `status_colors` JSON mapping of each configurable status to an opaque
`#RRGGBB` color, and the WP-96 `mfu_panel_meeting`/`mfu_panel_streaming`
booleans — one independent key per screen, each defaulting to `true`, gating
only that screen's MFU panel visibility, never Craft MFU itself), applied
immediately and across restarts. The React layer owns **theming** (light /
dark / system, plus release themes) and **i18n** (English default, release
languages); the OS scheme drives the _System_ theme.

`models/` manages a **fixed, app-defined catalog** of the model(s) each task
needs (transcription = Whisper, diarization = sherpa-onnx segmentation +
selectable embedding, MFU = llama/Qwen at M3). **Download** fetches from a
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
deferred model management" detail.

## Export

**As actually built, not as originally planned:** there is no `export.rs`
Rust module. A meeting's transcript (and, for Markdown, its MFU) is
rendered client-side and written to a user-chosen destination via the
generic `save_text_dialog(content, default_name)` command — `save_text_dialog`
itself is format-agnostic; it just writes whatever string it is given.

**Meeting export** (`src/export.ts`, WP-15/WP-24): a persisted
`export_file_type` setting (`"plain_text"` | `"markdown"`, Settings → Export)
selects the rendering. `renderForExport` is the one function both **Save**
(`handleSave`) and the header **copy** action (`CopyButton`,
`src/CopyButton.tsx`) call, so file export and clipboard copy can
never render differently. Plain text (`renderPlainText`) is unchanged from
before this setting existed — transcript only, `"Label: text"` per line, no
MFU. Markdown (`renderMarkdown`) adds a `# Transcript` heading, bold speaker
labels, `[m:ss]` timestamps, and — only when the meeting has MFU — a
`## MFU` section with one `### <field>` subsection per non-empty MFU
field.

Streaming's export/copy (WP-74, `StreamingView.tsx`) is a separate,
older implementation following the same real pattern — render client-side,
reuse `save_text_dialog` — but does not share `export.ts`'s rendering or its
file-type setting. Both screens' **Copy** buttons share `CopyButton`
(`src/CopyButton.tsx`), which calls `navigator.clipboard.writeText` directly
(no Tauri clipboard plugin was added — the web API works in the WKWebView and
avoids a new plugin/capability-permission surface for a one-line need) and
confirms a successful write with a transient checked button state plus a
top-center "Copied" toast that rolls back automatically after ~2.5
seconds;
**Export** always renders a minimal Markdown document (`# title` - the plain
transcript) through `save_text_dialog`. Both reuse `windowText`'s
`[unavailable]` marker for a fail-open window, so exported output matches
what the live view showed rather than silently dropping or blanking a failed
span.

## IPC Contract

| Command                                                                | Purpose                                                                                                                                                                                       | Milestone |
| ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------- |
| `open_file_dialog`                                                     | Pick a source audio/video file                                                                                                                                                                | M1        |
| `create_meeting()`                                                     | Create an empty meeting; returns its id                                                                                                                                                       | M2        |
| `attach_file(meeting, path)`                                           | Attach the source file to a meeting                                                                                                                                                           | M2        |
| `transcribe_meeting(id)`                                               | Transcribe the attached file into the meeting, then diarize it; no language argument — it is always detected (ADR-012). The invoke resolves when the run finishes; Meeting decode emits no percent-progress event.                                           | M2        |
| `list_meetings()`                                                      | Meetings list (summaries)                                                                                                                                                                     | M2        |
| `open_meeting(id)`                                                     | Full meeting (segments, MFU, meta)                                                                                                                                                          | M2        |
| `rename_meeting(id, title)` / `delete_meeting(id)`                     | Library management                                                                                                                                                                            | M2        |
| `update_segment(meeting, seg, text)` / `update_mfu(meeting, MFU)`  | Auto-saved edits                                                                                                                                                                              | M2/M3     |
| `export_meeting(id, format, target)`                                   | Write Markdown / plain text                                                                                                                                                                   | M2        |
| `list_models()`                                                        | Available (downloaded) Whisper models for the switcher                                                                                                                                        | M2        |
| `diarize_meeting(id)`                                                  | Produce + merge speaker turns                                                                                                                                                                 | M2        |
| `get_settings()` / `set_setting(key, value)`                           | Read/update settings (theme, ui_language, active model)                                                                                                                                       | M2        |
| `list_task_models()`                                                   | Per-task model catalog with download state                                                                                                                                                    | M2        |
| `download_model(id)` / `delete_model(id)`                              | Fetch (SHA-verified, progress) / remove a model                                                                                                                                               | M2        |
| `set_active_model(task, id)`                                           | Choose the active model for a task                                                                                                                                                            | M3        |
| `check_update()` / `apply_update()`                                    | App update                                                                                                                                                                                    | M3        |
| `generate_mfu(id)`                                                   | Generate structured MFU MFU (Create MFU)                                                                                                                                                    | M3        |
| `list_streaming_sessions()`                                            | Streaming sessions list (summaries)                                                                                                                                                           | WP-68     |
| `open_streaming_session(id)`                                           | Full session (all decoded windows)                                                                                                                                                            | WP-68     |
| `rename_streaming_session(id, title)` / `delete_streaming_session(id)` | Library management, mirroring Meeting's                                                                                                                                                       | WP-68     |
| `start_streaming_session()`                                            | Claim the shared Whisper context, create the session record, start mic+system-audio capture and the decode/persist loop; returns once capture starts (macOS only — errors on other platforms) | WP-68     |
| `stop_streaming_session()`                                             | Drop the held capture, cascading to end decode/persist and release the shared context (macOS only)                                                                                            | WP-68     |
| `generate_streaming_mfu(id)`                                           | Generate structured MFU for a Streaming session's transcript and persist it (Craft MFU)                                                                                                      | WP-77     |
| `generate_streaming_prettify(id)`                                      | Generate a cleaned-transcript candidate for review; not persisted until accepted                                                                                                              | WP-75     |
| `accept_streaming_prettify(id, text)`                                  | Persist an accepted prettify candidate                                                                                                                                                        | WP-75     |
| `revert_streaming_prettify(id)`                                        | Delete the accepted prettification, restoring the raw per-window transcript                                                                                                                   | WP-75     |
| `translate_streaming_paragraph(session_id, paragraph_key, target_language, text)` | Translate one Streaming paragraph into `"en"`/`"ru"` via the active summary LLM and persist it, keyed by `(session_id, paragraph_key, target_language)`; called by the Live Translation queue (WP-93) | WP-92     |
| `list_streaming_translations(session_id, target_language)`             | Read every persisted translation for a session and target language, so the Live Translation queue (WP-93) reuses stored results instead of re-running the model                              | WP-93     |

Events: `transcription_phase { id, phase: "diarizing" }` marks the transition
between the Meeting run's two passes; completion and errors return through the
`transcribe_meeting` invoke promise. `model_download_progress { id, fraction,
stage }` uses `stage` =
`downloading` while bytes arrive and `verifying` while the fetched file is
SHA-hashed, a pass long enough on a large model that the UI must name it rather
than show a full bar. `Segment` is
the shared transcript unit
(`{ id, start_ms, end_ms, text, speaker_id? }`). Errors are `AppError` serialized
to a human-readable string.

**Streaming events:** `streaming_window { session_id, window_index, start_ms,
end_ms, text, language, outcome_ok }` fires once per decoded window, whether
it succeeded or fail-open-skipped (`outcome_ok` distinguishes the two, same
convention as the persisted row). `streaming_sources { session_id, mic,
system_audio }` fires once, right after a session starts, naming which
capture source(s) actually came up — the UI's mic-only-degradation
indicator. `streaming_session_ended { session_id }` fires once the decode
loop has fully ended after `stop_streaming_session`.

## Security And Privacy

**Transcription and MFU detail generation make no network calls** — they run
entirely on-device, and there is **no telemetry**. The **only** networked
operation is **user-initiated model downloads** (and, at release, the app
update), fetched from known URLs and **SHA-verified**. Audio, transcripts, and
MFU never leave the device. File writes: the temporary ffmpeg WAV (deleted
after use), the SQLite library and the settings store under the app support
directory, downloaded model files, and user-chosen export destinations. No
`.env` or secret handling.

## Build MFU

- Tauri v2 + React 19 + TypeScript; Vite dev server on port 1420.
- `whisper-rs = { features = ["metal"] }`; `rusqlite = { features = ["bundled"] }`.
- ffmpeg on PATH. M2 adds sherpa-onnx (via the `sherpa-rs` crate, prebuilt
  binaries fetched at build time); M3 adds llama.cpp — both Metal, local.
- Streaming (WP-70) adds `cpal` (microphone) and `screencapturekit`
  (system-audio loopback), both macOS-only-target dependencies like
  `whisper-rs`'s `metal` feature — `cpal`'s default Linux backend
  (`alsa-sys`) needs ALSA dev headers CI does not install, and
  ScreenCaptureKit is Apple-only. Building `screencapturekit` needs the full
  Xcode.app installed (not just Command Line Tools) — see Streaming Audio
  Capture above.
- M2 adds an HTTP client for SHA-verified model downloads and a settings store;
  front-end gains theming (light/dark/system) and i18n (English default).
- Native dylib packaging (WP-60/WP-86): sherpa-rs-sys and dynamically linked
  llama-cpp place eight runtime libraries in the Cargo profile directory:
  sherpa-onnx, ONNX Runtime, and six llama/ggml dylibs. The linker records them
  as `@rpath/…`. `build.rs` links every binary with
  `@executable_path/../Frameworks` and `@executable_path` rpaths. After Cargo
  compilation and immediately before bundling, Tauri's `beforeBundleCommand`
  runs `scripts/stage-native-dylibs.sh`; the script stages all eight dylibs into
  the generated `src-tauri/frameworks/` for
  `bundle.macOS.frameworks` to copy into `Contents/Frameworks`. Without both, a
  packaged build aborts at dyld before `main` while `cargo run` keeps working,
  because cargo supplies a fallback search path the `.app` never gets.
  `src-tauri/tests/packaging.rs` asserts the config, staging, and rpaths. Tauri's
  local/debug bundle signs each staged dylib and deep signature verification is
  part of validation; release identity signing and Apple notarization remain a
  release operation rather than a build-time guarantee.
- Run: `npm install`, then `npm run tauri:dev`.

## Ownership

| Concern                                                   | Owner                                                                                               |
| --------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Tauri command layer (per-domain) + registration in `run()`| `src-tauri/src/commands/` + `src-tauri/src/lib.rs`                                                   |
| App state, model cache, running-run slots                  | `src-tauri/src/state.rs`                                                                            |
| IPC event payloads                                        | `src-tauri/src/events.rs`                                                                           |
| Audio normalize + decode                                  | `src-tauri/src/audio.rs`                                                                            |
| Whisper transcription + Streaming progress                | `src-tauri/src/transcribe.rs`                                                                       |
| SQLite meeting library                                    | `src-tauri/src/store.rs`                                                                            |
| Meeting persistence facade / DTOs                         | `src-tauri/src/meetings/` (`mod.rs` facade, `dto.rs` DTOs + coalesce)                               |
| Error type                                                | `src-tauri/src/error.rs`                                                                            |
| Model catalog + download                                  | `src-tauri/src/models/` (`catalog.rs` catalog + state, `download.rs` fetch/verify)                  |
| Speaker diarization                                       | `src-tauri/src/diarize/` (`clustering.rs`, `segmentation.rs`, `speakers.rs`, `pipeline.rs`)         |
| Diarization process isolation                             | `src-tauri/src/diarize_process/` (`transport.rs`, `worker.rs`, `supervise.rs`)                      |
| Two-pane shell: meetings list, meeting workspace, editors | `src/`                                                                                              |
| Streaming capture / decode / persistence / IPC facade     | `src-tauri/src/streaming_audio.rs` / `streaming_session.rs` / `streaming_store.rs` / `streaming.rs` |
| Streaming tab                                             | `src/StreamingView.tsx`                                                                             |
