# Meeting architecture

Part of the [architecture map](../architecture.md). Covers live system-audio
capture, rolling decoding, persistence, and the Meeting workspace.

## Audio Capture (WP-68/WP-70, `streaming_audio.rs`)

Meeting (ADR-014) is a separate capture mode from Transcription: near-
real-time transcription of live audio rather than a finished file. This
section covers only its capture layer (WP-70); the rolling-window
decode pipeline that consumes this module's output is WP-71 (see below).

Meeting uses system audio only and never opens or mixes the microphone.
`src-tauri/Info.plist` declares `NSScreenCaptureUsageDescription` for this
path; Recorder has a separate `NSMicrophoneUsageDescription`. Tauri
auto-merges both declarations into the app bundle, while each native source
still opens only from its corresponding explicit user action.

`streaming_audio.rs` receives an immutable capture specification before the
session starts and hands its consumer a continuous mono f32 stream through a
bounded `std::sync::mpsc::SyncSender`/`Receiver` pair. Local transcription,
Deepgram, and AssemblyAI request ScreenCaptureKit's native 16 kHz rate; OpenAI
requests its native 24 kHz rate. A background capture pump drains the single
system-audio buffer every 100 ms, independent of WP-71's 5–10s decode window.
The callback staging buffer is capped at four seconds at the highest supported
native rate; the downstream queue holds 600 chunks, nominally 60 seconds at
the 100 ms pump cadence. Its nominal sample payload is 3.84 MB at 16 kHz mono
f32 or 5.76 MB at 24 kHz; the channel is bounded by chunk count, so unusually
delayed pump ticks can make individual chunks larger. The backlog absorbs
first-load or committed-translation Metal stalls without losing the live
source. Both boundaries use an explicit drop-newest policy without blocking
the ScreenCaptureKit producer.
Each delivered chunk also carries its position on the original capture clock,
so dropped spans become explicit failed transcript windows instead of silently
compressing all later timestamps. There is no application resampling, downmix,
or two-source mixer.

Local decode results use a dedicated nonblocking result queue. It keeps only
the latest replaceable partial revision and at most 32 accepted committed
quality results or capture-gap records. When a committed window arrives it
removes the obsolete partial, so a slow renderer or SQLite write never blocks
the decoder. If persistence cannot drain 32 durable results, the next result
is returned to the decoder unsent and the queue emits an explicit terminal
overload signal after draining the accepted prefix; the runtime stops capture
and marks the session failed rather than silently dropping text or retaining
unbounded transcript memory. If Stop finds the bounded audio queue full, the
capture pump returns immediately and a detached sender delivers an explicit
zero-sample terminal-gap marker once queue space opens; the original capture
timeline is therefore never silently truncated.
The cloud relay (128 audio chunks) and provider-result queue (64 events) are
also bounded and propagate backpressure toward that first producer boundary.

The macOS-only **system-audio loopback** uses the `screencapturekit` crate.
Its `SCStreamOutputTrait::did_output_sample_buffer` callback reinterprets the
requested mono `AudioBufferList` bytes as little-endian f32 and forwards them
unchanged. `screencapturekit`'s mandatory `apple-metal`
dependency links `libswift_Concurrency.dylib`, an OS-provided Swift
runtime library that exists only in the dyld shared cache (no standalone
file, unlike the sherpa-onnx/onnxruntime dylibs WP-60 bundles) — `build.rs`
adds a fixed `-Wl,-rpath,/usr/lib/swift` link arg so the binary resolves it
without bundling, and `tests/packaging.rs`'s WP-60 regression test carries
an explicit `OS_PROVIDED_DYLIBS` exemption (plus its own rpath assertion)
for this dylib rather than treating it as one to bundle. Building this also
requires the full Xcode.app (not just Command Line Tools), for the Swift
compatibility libraries `apple-metal` needs at link time.

Because system audio is the sole source, inability to start ScreenCaptureKit
is a hard capture error rather than a microphone fallback. The pure capture
specification is tested at 16 and 24 kHz; granting real screen-recording
permission and exercising OS-level audio callbacks still requires manual
verification in the packaged app.

Mutual exclusion with an active Transcription run is WP-71's concern, not
implemented here — this module only captures audio, it does not
decode it.

## Meeting Decode/Session Pipeline (WP-68/WP-71, `streaming_session.rs`)

Decodes the continuous sample stream `streaming_audio.rs` produces into
non-overlapping logical utterances. After at least two seconds of speech, a
trailing 600 ms quiet run commits a natural phrase; the energy threshold adapts
to the preceding second of speech, so a noisy pause remains detectable after
both short and long utterances without classifying steady room noise as speech.
Uninterrupted speech is capped at 20 seconds (`WINDOW_SECONDS`) and uses
a short quiet boundary near that cap when available. This avoids mechanical
seven-second word splits while keeping memory and latency bounded.
A conservative energy/stationarity gate prevents confidently steady low-level
noise from triggering partials, Stop-tail decode, or a forced hard-cap commit;
a noise-only hard-cap span is discarded while its sample-clock time is retained.
Varying low-gain audio fails safe toward ASR so quiet speech is not deleted.
Once the unstable suffix reaches two seconds, local decode emits a transient
partial and revises it for each additional two seconds. Replaceable partials
use deterministic greedy decoding; committed, stop-tail, Recorder-final and
whole-file Whisper passes retain beam-5 decoding with guarded temperature
fallbacks. Partials are sent as `streaming_partial` events and never persisted.
The Meeting renderer treats the complete replaceable hypothesis as unstable
and renders it italic at 80% opacity. No prefix becomes visually final before
the backend commits the acoustic window. A committed window replaces the
hypothesis with one normal-weight committed
`streaming_window`. On Stop, a meaningful trailing suffix of at least 500 ms
is decoded and committed exactly once; silence and sub-threshold callback
noise are discarded. One session decodes every result through a
single `WhisperState` (WP-82): `run_windowed_decode` builds one
`WhisperSessionDecoder` when the loop starts and reuses it via
`transcribe::transcribe_with_state_and_prompt_profile`, because each state owns a full GPU
backend plus its KV/compute buffers — one state per window was one backend
init/free cycle per window. State reuse across calls is upstream's own
`whisper_full` pattern (each call clears results, recomputes the mel, and
clears the self-attention KV cache); Transcription keeps one state per whole-file
run. Up to the last 240 characters of committed text are supplied as context
to both Whisper and Qwen, improving proper nouns and continuity without
persisting or duplicating the prompt. Qwen receives that text inside an
unmodified, always-present system-context message (empty for the first window),
while its user message contains only the audio marker. This follows Qwen3-ASR's
upstream prompt contract and avoids injecting general-purpose English
instructions into the ASR decode.
Whisper prepends a short Russian/English punctuation exemplar to that rolling
context. Punctuation is decoded by Whisper rather than rewritten afterward;
the seed prevents an early unpunctuated greedy hypothesis from becoming the
style prompt for every later window without changing Transcription, Recorder, or
Qwen.
Each window gets its own language detection, unlike Transcription's
once-per-file detection (ADR-012), since a live session has no single fixed
language the way a finished file does. After a forced hard boundary, the next
decode prepends 750 ms of the previous audio while its persisted logical span
still begins at the non-overlapping sample boundary. Whisper removes only full
segments whose model timestamps prove they fall wholly inside those 750 ms; a
boundary-crossing segment is preserved. Qwen exposes no reliable model
timestamps, so ambiguous repeated text is always preserved and its explicit
context instruction remains the only duplicate-avoidance hint.

**Fail-open per window** (mirroring diarization, ADR-013): a window whose
decode errors is skipped — logged, no text emitted for that span — rather
than ending the session. `WindowResult.outcome` carries the `Result` through
rather than the loop propagating it.

Capture-clock discontinuities are persisted as failed windows. If a meaningful
prefix before a discontinuity is shorter than the 500 ms minimum decode tail,
that prefix is folded into the failed span rather than silently discarded.

**Mutual exclusion** (`WhisperUsageGuard`, backed by a new `AppState.
whisper_busy: AtomicU8`): a Transcription run and a Meeting
cannot run concurrently. Both use the one cached Metal context, with a fresh
Whisper state for each whole-file Transcription run and a reused state for each
Meeting. Concurrent native model work remains intentionally
serialized. `transcribe_meeting` acquires this guard for its whole duration,
released on drop;
Meeting's session start claims the same guard before model load and keeps the
same `recorder_asr_mutation` barrier from local ASR selection through that
successful claim. This prevents a concurrent model-selection/deletion change
from observing an idle decoder and removing a bundle that Meeting is starting.
This
guard is deliberately not narrowed to "only block the _other_ kind" — two
concurrent Transcription runs are serialized by the same guard, a small,
disclosed safety tightening beyond WP-68's literal Meeting-vs-Transcription
ask, since the underlying safety property (one decode at a time against the
shared context) does not depend on which caller is asking.

Wired to Tauri IPC via `start_streaming_session`/`stop_streaming_session` in
`commands/streaming.rs` (WP-73) — see the Meeting Runtime & UI section below for the
command/event layer that ties this module to `streaming_audio.rs` and
`streaming_store.rs`.

**Latency is not measured against real hardware.** The feasibility spike
WP-68's own DoD requires — measuring real per-window decode latency across
supported Mac hardware — needs a person running this with a downloaded
model, which this environment cannot do. `WindowResult.decode_ms` exists so
that measurement is possible once someone can run it; `WINDOW_SECONDS`
itself is not yet the finalized, measured threshold.

## Meeting Persistence (WP-68/WP-72, `streaming_store.rs`)

A legacy `StreamingSession` entity (Meeting UI) is separate from the legacy
`Meeting` entity (Transcription UI; WP-68 D5): no
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
Sidebar duration is not inferred from `updated_at_ms - created_at_ms`:
`list_sessions` derives it from the maximum persisted `end_ms`. This keeps a
cleared session at zero and a resumed session aligned to its new audio timeline
regardless of wall-clock age.

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
Meeting tab consumes.

`streaming_translations` (WP-92) is the newest table in this store, keyed by
`(session_id, window_index, target_language)` with `ON DELETE CASCADE` on
`session_id`, created in the same `CREATE TABLE IF NOT EXISTS` schema batch as
`streaming_prettified`. `window_index` was originally the `window_index` of a
_paragraph's_ first window (one row per paragraph); ADR-016/WP-103 renamed
the column via a checked `ALTER TABLE … RENAME COLUMN` migration when the
translation unit moved to a single window (one row per window — see Live
Translation below). `StreamingStore::upsert_translation` follows
`append_window`'s `ON CONFLICT … DO UPDATE` idiom, so a repeated translate for
the same key overwrites in place rather than duplicating; `list_translations`
reads back every stored translation for one session and target language. Each
row also stores the source text it was translated from, so
`StreamingTranslation::is_stale(current_source_text)` can tell a caller when
that window's text changed (e.g. a fail-open retry) and the stored
translation no longer matches — re-translation, not display, is then the
caller's job. See Live Translation below for the local-LLM call and command
that populate this table.

`StreamingStore::clear_session_content` rejects an active session and deletes
segments, translations, MFU, and accepted Prettify output in one transaction.
The session row, title, engine provenance, and translation preferences remain,
so the cleared history item can be resumed without stale derived content.

## Meeting Runtime & UI (WP-68/WP-73, `start_streaming_session` /

`stop_streaming_session` in `commands/streaming.rs`, `src/StreamingView.tsx`)

Starting a session ties `streaming_audio.rs` (capture), `streaming_
session.rs` (decode/mutual-exclusion), and `streaming_store.rs`
(persistence) together through `whisper_busy` and the backend-owned
`LiveCaptureRuntimeCoordinator`. The coordinator holds the macOS capture
runtime and a typed `idle → starting → capturing → stopping/error` snapshot
with monotonic generation and revision numbers. `get_live_capture_snapshot`
plus `live_capture_state` let any newly mounted renderer subscribe first and
then reconcile the current snapshot without an event/query race. View changes,
hidden windows and webview remounts therefore cannot orphan capture or unlock
capture-sensitive Settings controls. Those controls fail closed until the
initial snapshot has hydrated. Stop remains available while the backend is in
`starting`; cancellation then stays visibly `stopping` until any asynchronous
setup resource has been released and the session's stopped status is durable.

`start_streaming_session` claims `whisper_busy`, then either creates a fresh
session row (no `session_id` argument) or **resumes** a previously-stopped
one: given a `session_id`, `streaming::resume_streaming_session` validates
the session exists and is `STOPPED` (rejecting an already-`ACTIVE` one —
resuming it would double-capture), computes the window index to continue
counting from (one past the last persisted window, or 0 if none was ever
saved) and the last persisted `end_ms`, and flips its status back to `ACTIVE`
via the new `StreamingStore::mark_active`. Either way it then starts capture
and spawns two `spawn_
blocking` tasks: one runs `run_windowed_decode_from` with the persisted index
and timeline offset so both indexes and sample-derived timestamps continue
strictly after the prior take, the other (`drive_streaming_
results`) consumes its `WindowResult`s — persisting each via `append_window`
and emitting `streaming_window` — until the results channel disconnects, at
which point it marks the session stopped and releases `whisper_busy`.
`src/StreamingView.tsx`'s Start action resumes the currently-open session
when it's a past, stopped one (and relabels itself "Resume" accordingly),
unless the user selects a different transcription engine. Session engine
configuration is immutable for transcript provenance, so that switch starts a
new session instead of resuming a Cloud session as Local (or vice versa). The
header's "+"/New icon always starts fresh regardless of what's open, via a
separate `handleStartNew` that never passes a `session_id`.
`stop_streaming_session` atomically moves the coordinator to `stopping`, takes
its runtime and lets it drop. Dropping the held `streaming_audio::
StreamingSession` stops the system-audio stream, which cascades through the
capture pump → sample channel → decode loop → results channel, ending
`drive_streaming_results` on its own. `SCStream` (`screencapturekit`) is
`Send`/`Sync` (explicitly documented in the crate), so storing the capture in
`AppState`'s standard mutex needs no additional unsafe code.

On non-macOS targets, `start_streaming_session`/`stop_streaming_session`
are still registered (same command names, same generated-handler list) but
return a "macOS only" error — keeping the frontend's command surface
identical across platforms rather than branching on `cfg` in TypeScript.

`src/StreamingView.tsx` is the self-contained top-level Meeting view
(entered/exited like `SettingsScreen`), separate from the Transcription workspace — but
since this UI redesign it now mirrors `App.tsx`'s window shell wholesale
(`.wp-header`/`.wp-info-bar`/`.wp-sidebar`/`.wp-transcript-panel`/`.wp-mfu`,
plus the shared `ActionIcon` component) rather than keeping its own bespoke
sidebar/action-row markup, so the two windows can't drift apart visually. The
two windows are switched via a `ModeToggle` control in each one's sidebar
(`src/ModeToggle.tsx`, "Transcription" / "Meeting" segments) instead of a
dedicated header icon — clicking the inactive segment calls `onClose` (from
Meeting) or opens Meeting (from Transcription). Because `StreamingView` is a
mounted child rather than inline JSX in `App`, opening Settings from inside
it (its header also carries the same Toggle-sidebar/New/Settings icon group
as Transcription's) renders `SettingsScreen` as a fixed-position `.settings-overlay`
layered on top rather than replacing the tree — swapping to `SettingsScreen`
outright would unmount `StreamingView` and drop a live recording's in-flight
state; the overlay keeps it mounted underneath. `StreamingView` owns: the
session list (rename/delete, styled with legacy `wp-meeting-row` classes,
mirroring Transcription's), Start/Stop/Craft/Copy/Export/Delete in the header's Main Actions
row, a live transcript that appends `streaming_window` events for whichever
session is currently open (`upsertWindow` replaces rather than duplicates a
resent `window_index`, and ignores events for a session that isn't the open
one — stale events from a just-stopped session are possible during the
transition), an Audio Source chip in the info bar for the `streaming_sources`
indicator showing the active System Audio source, a header
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

Rename uses the in-app `.modal-overlay`/`.modal-panel` form. Delete, Clear, and
simple confirmation boundaries share `src/ConfirmDialog.tsx`; it owns focus,
Escape cancellation, the modal action vocabulary, and the destructive error-
color variant. The UI never relies on `window.prompt`/`window.confirm`, which
Tauri's WKWebView does not reliably wire up (they silently no-op rather than
showing anything).

The raw live transcript groups windows into `<p>` paragraphs via `src/
paragraphs.ts`'s `groupWindowsIntoParagraphs` — a client-side sentence-
boundary + length heuristic. A paragraph preferentially closes at terminal
punctuation after either the soft character target or four acoustic windows.
If ASR emits no terminal punctuation at all, a twelve-window hard ceiling keeps
the render tree and visible block bounded; this is the only path allowed to cut
without a semantic boundary. Windows carry no pause/VAD signal
of their own to split on (they're fixed-size slices of continuous audio, not
silence-delimited — see Meeting Decode/Session Pipeline above). Each
window keeps its own span (fail-open styling, per-window tooltip) inside its
paragraph, so this is purely a rendering grouping, not a change to the
underlying transcript data. Prettify's LLM cleanup does not yet also emit
paragraph breaks — a possible follow-on, not yet built.
