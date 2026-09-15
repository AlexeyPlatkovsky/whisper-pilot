# Recorder architecture

Part of the [architecture map](../architecture.md). Covers microphone capture,
durability, transcript processing, and persisted recordings.

## Runtime And Storage (ADR-017)

Recorder is a third Rust-owned live source beside Meeting, not a microphone
fallback inside `streaming_audio.rs`. The shared live-capture coordinator now
owns a typed Meeting-or-Recorder native runtime slot. Recorder commands reuse
it with the Whisper ownership guard so only one of Transcription,
Meeting capture, Recorder capture, or Recorder finalization
can own native transcription resources. Renderer mount state and the visibility
of the main or caption window are never lifecycle authorities.

The explicit **New recording** action creates a durable metadata-only draft
immediately. It has no audio artifact and survives launch reconciliation, so it
can be selected, renamed, or deleted before capture. Start runs macOS microphone
authorization, the draft's selected ASR model bundle, and system-default input
device preflight before activating that same row in place. A failed start
restores the non-live draft state before cleaning temporary audio, so even a
filesystem cleanup failure cannot leave a phantom active session.
Starting a completed row continues it in place. The writer copies its valid
final CAF into a new `.partial`, appends at the same native sample rate, and
atomically replaces the final CAF only after Stop. Existing segments remain
visible while recording; new window timestamps start at the prior audio
duration, and the final quality pass retranscribes the combined master. A
potentially large CAF copy runs on the blocking worker pool rather than a Tauri
async command executor, so continuing a long note does not stall unrelated IPC.
A different current microphone sample rate is rejected before capture instead
of producing a malformed concatenation. Failed startup removes only the new
partial and restores the completed row and any accepted polish.
After interruption, the normal Recover action validates and promotes the
combined partial over the still-safe old final; invalid partial data leaves the
old final untouched. If failed-start cleanup itself cannot remove the partial,
the row remains Recoverable rather than claiming the exactly-one-file Completed
state.
Shortcut Start has no preselected draft and retains the stricter boundary: no
row or audio exists until preflight succeeds. A typed AVFoundation adapter exposes `not_determined`,
`denied`, `restricted`, `authorized`, and `unavailable` over IPC. Its status
query never opens a device or prompts; only the explicit Recorder request
command may display TCC, and capture fails closed unless the resulting status is
`authorized`. The macOS microphone adapter uses CPAL 0.17.3 at the device's
default PCM format. Its callback converts integer or floating-point
samples to f32 and averages interleaved channels into an ordered mono master at
the device's actual rate without waiting for ASR. Callback storage comes from a
bounded, preallocated recycle pool; downstream receives only a read-only slice,
and releasing a chunk returns a capacity-checked buffer to the pool. Exhaustion
stops delivery as an explicit overload instead of allocating on CoreAudio's
real-time thread. The native-rate CAF writer is the durability boundary and
never waits for realtime ASR: its derived 16 kHz feed uses non-blocking
drop-newest delivery when the decoder falls behind. Such preview gaps do not
make the recording recoverable because the post-stop quality pass rebuilds the
complete transcript from the preserved master. The CAF writer encodes that
master as signed-16 PCM at the same declared rate. Independent, band-limited adapters derive the 16 kHz
local-ASR stream, the 24 kHz OpenAI Realtime stream, or a provider-supported
native-rate stream. Recorder never mixes microphone and system audio. A change
to the system default does not migrate an active stream; CPAL interruptions and
bounded-queue overload are surfaced once so the coordinator can end capture as
a recoverable error.

ASR selection is capability-driven (ADR-019), not inferred from catalog order.
The static model specification declares engine, runtime, compatible modes,
streaming, timestamps, language detection, mixed-language support, memory,
license, and an exact asset fingerprint. `active_model.transcription` is the
single selection for Transcription, local Meeting, and Recorder. Older or unknown
selections normalize back to Whisper. Qwen3-ASR 1.7B Q8_0 is accepted for all
three modes with automatic language detection. Unsupported combinations and
incomplete bundles fail before capture, without implicit fallback.

Whisper retains its existing Metal context. Qwen 1.7B uses the existing
in-process `llama.cpp` backend plus MTMD, with a verified text GGUF and required
audio projector GGUF. Text LLM and GGUF-ASR caches share the one process-global
`llama.cpp` backend but own independent models and inference contexts. Transcription
feeds the GGUF runtime bounded 30-second decode windows. Every window after
the first advances the logical timeline by 29 seconds and includes one second
of preceding audio plus up to 240 characters of confirmed transcript context.
Because Qwen has no model timestamps, the pipeline preserves ambiguous repeated
phrases instead of risking deletion. Meeting and Recorder reuse the live
natural-utterance decoder with a 20-second hard cap. Qwen output has no model
timestamps, so the surrounding pipeline assigns stable window spans rather
than presenting them as word timing.
Every new Recorder row stores `asr_model_id`, `asr_engine`, and `asr_language`, preserving active
session immutability and historical provenance across settings changes.
A single async mutation barrier spans ASR selection changes, model loading, and
deletion. Any active Transcription, Meeting, or Recorder pipeline blocks model
mutation. Successful Qwen deletion also invalidates both Qwen caches; deleting
a selected multi-asset bundle first resets every affected mode to Whisper, and
a partial deletion keeps that safe selection until the bundle is complete.

An observable `finalizing` session state sits between capture and completion.
It retains live-source ownership while meaningful tail audio is transcribed,
the `.caf.partial` file is flushed, synced and closed, the file is atomically
renamed to `.caf`, and the saved native-rate audio is decoded once more with
the selected model's quality path. That second pass atomically replaces the
provisional live segments; if it fails, the live transcript remains usable and
the finalized audio still completes. The database row then becomes completed. The writer checkpoints
with at most one second of PCM not yet durable. Startup reconciliation preserves
and exposes mismatched database/filesystem states rather than deleting them. See
ADR-017 for the format, recovery, export, retention, and deletion contract.

The global shortcut callback lives in Rust and addresses the same
coordinator as the main window. Key repeat and stale callbacks cannot create more
than one transition. A non-activating caption window is a view of coordinator
state; hidden webview timers do not drive capture. If the required caption surface
disappears during a shortcut-started hidden recording, the backend requests Stop
and completes the same finalization path instead of leaving invisible capture.

Recorder polishing uses the same local-LLM scheduler and safety validators as
Meeting Prettify. Generation returns a review candidate only. Acceptance
upserts `recorder_polished`, leaving timestamped `recorder_segments` unchanged;
revert deletes the derived row and immediately restores raw display/copy/export.
Session deletion cascades to the derived row.
Clearing an inactive recording atomically renames final and partial CAF
artifacts into same-volume `.clearing` quarantine files under an immediate
SQLite transaction. A rename or database failure restores every moved file;
commit resets the retained row to an empty draft before quarantine deletion.
Launch reconciliation restores pre-commit quarantine for a non-draft row and
removes post-commit quarantine for a draft. Active capture is rejected, and
failed filesystem cleanup does not clear the database content.
