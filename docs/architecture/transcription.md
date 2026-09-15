# Transcription architecture

Part of the [architecture map](../architecture.md). Covers the full-file
Transcription workflow, persistence, decoding, and speaker diarization.

## Model & Persistence (`store.rs`, legacy `Meeting` identifiers)

The library is a local **SQLite** database (`whisperpilot.sqlite3` in the app
support directory, via bundled `rusqlite`). WP-16 implements the idempotent
schema and Rust CRUD store. WP-21/WP-22 expose create/list/open/rename/delete
commands and hydrate the workspace from persisted transcriptions. A
**Transcription** is one source file and its derived text. Transcriptions
**reference the original file path** — audio is not copied — so a Transcription whose
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
| `meetings` | `id`, `title`, `source_path`, `source_name`, `created_at_ms`, `duration_ms`, `language`, `status` |
| `segments` | `meeting_id` + `ordinal` (composite key), `start_ms`, `end_ms`, `text`, `speaker_id` (M2)         |
| `MFU`      | `meeting_id`, `summary`, `decisions`, `action_items`, `open_questions`, `participants` (M3)       |

The store replaces/reads segments in ordinal order, upserts a single MFU
record per Transcription, and cascades Transcription deletion to dependent rows. Segment
text and MFU edits are **auto-saved** (WP-17): the front end debounces each
edit (500ms idle) and calls `update_segment`/`update_mfu`, which persist to
the DB immediately; pending writes retain both their legacy `meeting_id` and immutable
payload, so switching or clearing another Transcription cannot cancel or misroute
them. Clear drains only the target Transcription's pending writes before deleting its
derived content. A failed background write is surfaced only while its owning
Transcription is still open, never as the status of a different Transcription. There is no
explicit save state or button. Export is a separate, explicit write to an
external file.

Stored `segments` rows start as whisper's original fine-grained spans —
`to_dto` (`meetings/dto.rs`, WP-48) coalesces consecutive same-speaker rows into
larger display blocks on every read path (see Speaker Diarization below), and
that is what the UI renders and edits as one block. `update_segment` (WP-17)
therefore writes back at the same granularity the user edited: it re-derives
the current coalesced list, replaces the edited block's text, and rewrites
_all_ segment rows for the Transcription to match it. This means editing any one
coalesced block also collapses the other, untouched blocks in that Transcription to
one row per display block from that point on — the fine-grained pre-edit
ordinals are not preserved once an auto-save has happened. Speaker labels
(the user-facing rename in `SpeakerLabelEditor`) remain session-only, not
persisted; only segment text and MFU are in WP-17's scope.

## Audio Ingestion (`audio.rs`)

Any input — audio or video — is normalized through **one** path: ffmpeg emits
16 kHz mono signed-16-bit PCM (`-vn -ac 1 -ar 16000 -f s16le`) to stdout, and
`audio.rs` converts those bytes directly into normalized f32 samples. There is
no synthetic in-memory WAV copy and no temporary audio file. An incomplete
final PCM sample is rejected rather than truncated. ffmpeg extracts audio from
video and resamples audio identically, so no branch on file type is needed.
ffmpeg is a required external dependency (system binary on PATH for now).

## Transcription (`commands/transcription.rs`, `transcribe.rs`, `qwen_gguf_asr.rs`)

`transcribe_meeting` resolves the shared active ASR selection and then uses one
of two local engines. Whisper runs through whisper-rs with Metal; its context is
created once and cached in `AppState`, and the normalized file is decoded with
beam search. Qwen3-ASR 1.7B runs through the local GGUF adapter in 30-second
logical windows. Every Qwen window includes one second of preceding audio and
the recent confirmed text as context; placement offsets model-relative times by
the decode start, discards output wholly inside the overlap, clips a
boundary-crossing segment to the logical range, and suppresses an exact repeated
boundary segment. Both branches return timestamped `Segment`s and incremental
progress, then use the same atomic persistence and diarization path.

**Language is always auto-detected and can never be chosen** (ADR-012): no
renderer call supplies a language. Whisper reads the detected code from decoder
state; Qwen returns its detected language with each window and the first
non-`auto` result becomes the file language. The value stored in
`meetings.language` is therefore an _output_ of a run rather than an input.
Whisper detection uses the first 30 seconds of audio, so a recording that opens
with silence can misdetect — a known limitation.

Forcing a language is deliberately unreachable rather than merely defaulted:
decoding audio as a language it is not in makes Whisper emit one hallucinated
line per 30-second window instead of the transcript. `DecodeSettings` names the
configuration as plain data so it can be asserted in a unit test — notably
`detect_language_only`, which must stay `false` because whisper.cpp returns
immediately after detection when it is set, yielding an empty transcript.

Whisper emits callback progress and Qwen emits completed-window progress as
`transcription_progress { id, percent }` during the transcription phase.
Meeting retains its separate state-reuse path. Diarization has no percentage
estimate, so that second phase remains indeterminate.

On macOS, `llama-cpp-2` is dynamically linked while Whisper remains static.
Both engines embed incompatible ggml versions with the same exported symbol
names; linking both statically can resolve Whisper's Metal calls to llama.cpp's
ggml implementation and corrupt encoder output. The llama/ggml dylibs are
staged into the app bundle alongside the sherpa-onnx native libraries.

The **Transcribe** run is a two-phase pipeline: transcription, then **diarization

- merge** (M2, `diarize/`). The transcript is persisted between the two phases,
  so the Transcription is already marked finished while diarization is still running (see
  Speaker Diarization below). A progress spinner spans both phases; Transcription
  runs to completion and the isolated worker design for restoring
  Stop is tracked separately (WP-87). If diarization is unavailable or fails,
  the run still finishes with plain (speaker-less) segments. Re-running
  Transcribe on an item that already has
  a transcript replaces it (and any MFU) after a confirmation.

The decoded `Vec<f32>` is moved into the blocking Whisper task and returned
from that task for diarization; the whole recording is not cloned merely to
satisfy `spawn_blocking` ownership. The allocation remains the same across
the two stages until it is staged for the isolated diarization worker.

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
alone on an already-transcribed Transcription, without re-transcribing. Re-decodes
the source audio (samples are never persisted) and re-runs the same
`diarize_process::diarize_with_fallback` pass `transcribe_meeting` uses
internally, then assigns the resulting turns onto the Transcription's
already-persisted segments by time overlap
(`meetings::diarize_meeting_segments` → `diarize::assign_speaker_ids`) and
saves the result. Unlike the diarization folded into a Transcribe run, a
failure here is a real error surfaced to the user, not a fail-open warning —
diarization is what was explicitly asked for, so there is no already-safe
transcript to fail open onto. Requires an active diarization model, an
existing transcript, and a readable source file (disabled client-side
otherwise); reuses the same time-overlap assignment as the automatic pass,
so it re-diarizes at whatever segment granularity currently exists —
full per-utterance precision on a never-diarized Transcription, or the coarser,
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
output frames. Overlapping input windows are produced lazily in batches of at
most 32, including a zero-padded final partial window. This bounds temporary
segmentation input to one inference batch instead of materializing every
overlapping window for a long recording at once. The implementation pins
`ort` v1.16.3 because its declared Rust
1.70 minimum is compatible with this project's Rust 1.80 toolchain;
the current `ort` 2.x line requires a newer compiler.

Route-A quality is measured rather than inferred. The ordered 0.95 → 0.75
sweep over the two known-two-speaker reference recordings completed without a
native abort: the 861.57-second recording reaches two clusters at 0.85 and
0.80, while 0.75 produces three; the 92.47-second recording still merges to one
cluster throughout the approved range. The user selected 0.85 as the accepted
automatic clustering threshold; it reaches the long recording's known count
without the 0.75 over-clustering outcome.
The measured sweep is retained in the
[WP-62 completion evidence](../../.taskpilot/comments/WP-62/2026-07-28T06-47-34Z.md).

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
avoid filling the pipe buffer. Both parent serialization and worker
deserialization use a fixed 16 KiB byte buffer rather than constructing a
second recording-sized byte vector. The child is killed on an **inactivity**
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
(see Transcription Model & Persistence above) — coalescing is display-only.

**Diarization now runs automatically as part of `transcribe_file`** (WP-31):
audio is decoded once and both transcription and diarization run over the
same samples (each on its own `spawn_blocking`, off the async reactor);
`diarize_samples` auto-detects the speaker count (no override parameter
exposed yet) and, on success, `assign_speaker_ids` writes each segment's
`speaker_id` in place. Diarization failure of any kind — missing models, an
engine error, or the blocking task itself panicking — is fail-open: it is
logged and the transcription still returns its (speaker-less) segments,
never failing `transcribe_file`. Transcription has no in-process Stop;
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
consequences follow: the Transcription's stored status reads `finished` while
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
restarts (that requires persistent speaker labels on the Transcription entity). This
completes epic WP-1 (M2 speaker-attributed transcription).
