# Platform architecture

Part of the [architecture map](../architecture.md). Covers settings, model
management, export, and the IPC surface.

## Settings & Model Management (`settings.rs`, `models/`) — M2 beta, M3 release

Settings live in a small **key–value store** in the app support directory
(theme, `ui_language`, one shared active ASR model, task-specific diarization
and text models, export file type, the
WP-88 `status_colors` JSON mapping of each configurable status to an opaque
`#RRGGBB` color, the WP-96 `mfu_panel_meeting`/`mfu_panel_streaming`
booleans — one independent key per screen, each defaulting to `true`, gating
only that screen's MFU panel visibility, never Craft MFU itself — and the
WP-106 non-secret `cloud_provider` identifier), applied
immediately and across restarts. The React layer owns **theming** (light /
dark / system, plus release themes) and **i18n** (English default, release
languages); the OS scheme drives the _System_ theme.

`models/` manages a **fixed, app-defined catalog** of the model(s) each task
needs (transcription = Whisper plus optional Qwen3-ASR 1.7B GGUF,
diarization = sherpa-onnx segmentation +
selectable embedding, MFU = llama/Qwen at M3). **Download** fetches from a
known URL, streams progress, and marks a model ready only after **SHA
verification**; **Delete** removes the local file. A task whose required model
is absent is disabled or degrades (Transcription requires the complete,
currently selected local ASR bundle and never silently falls back;
diarization degrades per F002-R7). `active_model.transcription` selects one
local ASR for Transcription, Meeting, and Recorder work. Diarization has its own
multi-entry selection (WP-52): its catalog
entry already holds one shared segmentation asset plus multiple
independently-downloadable embedding variants (CAM++, TitaNet-large), addressed
by a synthetic `"diarization-<variant>"` id, with an `active_model.diarization`
setting selecting which embedding is Active (or `"none"` to skip diarization
entirely — the default for every user, including those upgrading from before
this selection existed). This supersedes the earlier "manual model placement /
deferred model management" detail.

Local text models now expose explicit profiles while retaining one execution
stack. Qwen3.5 4B Q4_K_M is the recommended **Fast** profile for fresh installs;
Qwen3.8 9B Q6_K and Gemma 4 12B QAT Q4_0 are opt-in **Quality** profiles. The
previous Qwen3 file remains a valid Legacy selection and no upgrade downloads
a replacement. Catalog metadata owns each model's profile, context,
sampling, prompt protocol, minimum-memory guidance, license label, exact file
size and SHA-256. `LlmRuntime` continues to serialize work through its bounded
priority queue and caches only the exact selected file fingerprint.

Qwen3.5/Qwen3.8 use pinned ChatML with `/no_think` plus a completed empty-think
assistant prefill. Gemma 4 uses the canonical no-thinking `<|turn>` protocol
from its pinned GGUF metadata because the compact llama.cpp C template renderer
does not execute that model's full Jinja. All result paths strip reasoning,
channel, fence and end-of-turn syntax. MFU accepts string/array/object field
forms but normalizes them back to the existing five-string DTO and stops on the
first balanced JSON object. Translation and polishing reject destructive length,
language, number and identifier changes. The qualification evidence and exact
hardware results are in `validation/phase-3-llm-qualification.md`.

Large downloads are restart-safe: a valid `.part` length is retained after a
network failure, free space is checked against only the remaining bytes, HTTP
Range resumes the transfer, and only a matching final SHA is atomically renamed
into place. Oversized or hash-mismatched partials are removed.

## Export

**As actually built, not as originally planned:** there is no `export.rs`
Rust module. A Transcription's transcript (and, for Markdown, its MFU) is
rendered client-side and written to a user-chosen destination via the
generic `save_text_dialog(content, default_name)` command — `save_text_dialog`
itself is format-agnostic; it just writes whatever string it is given.

**Transcription export** (`src/export.ts`, WP-15/WP-24): a persisted
`export_file_type` setting (`"plain_text"` | `"markdown"`, Settings → Export)
selects the rendering. `renderForExport` is the one function both **Save**
(`handleSave`) and the header **copy** action (`CopyButton`,
`src/CopyButton.tsx`) call, so file export and clipboard copy can
never render differently. Plain text (`renderPlainText`) is unchanged from
before this setting existed — transcript only, `"Label: text"` per line, no
MFU. Markdown (`renderMarkdown`) adds a `# Transcript` heading, bold speaker
labels, `[m:ss]` timestamps, and — only when the item has MFU — a
`## MFU` section with one `### <field>` subsection per non-empty MFU
field.

Meeting's export/copy (WP-74, `StreamingView.tsx`) is a separate,
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

| Command                                                                                            | Purpose                                                                                                                                                                                                                                                                                                                                            | Milestone             |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------- |
| `open_file_dialog`                                                                                 | Pick a source audio/video file                                                                                                                                                                                                                                                                                                                     | M1                    |
| `create_meeting()`                                                                                 | Create an empty Transcription; returns its id (legacy command name)                                                                                                                                                                                                                                                                                 | M2                    |
| `set_meeting_source(id, path?)`                                                                    | Attach or detach the source file for a Transcription (legacy command name)                                                                                                                                                                                                                                                                          | M2                    |
| `transcribe_meeting(id)`                                                                           | Process the attached file, emit `transcription_progress`, then diarize it; no language argument — it is always detected (ADR-012). The invoke resolves when the run finishes.                                                                                                                                                                     | M2                    |
| `list_meetings()`                                                                                  | Transcriptions list (summaries; legacy command name)                                                                                                                                                                                                                                                                                               | M2                    |
| `open_meeting(id)`                                                                                 | Full Transcription item (segments, MFU, meta)                                                                                                                                                                                                                                                                                                      | M2                    |
| `rename_meeting(id, title)` / `delete_meeting(id)`                                                 | Library management                                                                                                                                                                                                                                                                                                                                 | M2                    |
| `clear_meeting(id)`                                                                                | Clear Transcription segments and MFU while retaining the library item and attached source                                                                                                                                                                                                                                                          | M2                    |
| `update_segment(meeting, seg, text)` / `update_mfu(meeting, MFU)`                                  | Auto-saved edits                                                                                                                                                                                                                                                                                                                                   | M2/M3                 |
| `save_text_dialog(content, default_name)`                                                          | Let the renderer export prepared transcript or MFU text through the native save dialog                                                                                                                                                                                                                                                             | M2                    |
| `diarize_meeting(id)`                                                                              | Produce + merge speaker turns                                                                                                                                                                                                                                                                                                                      | M2                    |
| `get_settings()` / `set_setting(key, value)`                                                       | Read/update settings (theme, ui_language, active model)                                                                                                                                                                                                                                                                                            | M2                    |
| `list_task_models()`                                                                               | Per-task model catalog with download state                                                                                                                                                                                                                                                                                                         | M2                    |
| `download_model(id)` / `delete_model(id)`                                                          | Fetch (SHA-verified, progress) / remove a model                                                                                                                                                                                                                                                                                                    | M2                    |
| `get_cloud_provider_config()` / `select_cloud_provider(provider)`                                  | Read the fixed provider/model catalog and persist only the selected non-secret provider id                                                                                                                                                                                                                                                         | WP-106                |
| `verify_cloud_provider_api_key(provider, api_key)`                                                 | Check provider authentication/model access without saving or returning the key, and without captured audio                                                                                                                                                                                                                                         | WP-106                |
| `save_cloud_provider_api_key(provider, api_key)` / `remove_cloud_provider_api_key(provider)`       | Verify then add/replace, or delete, exactly one provider Keychain credential; responses contain status metadata only, never the key                                                                                                                                                                                                                | WP-106                |
| `generate_mfu(id)`                                                                                 | Generate structured MFU (Create MFU)                                                                                                                                                                                                                                                                                                               | M3                    |
| `list_streaming_sessions()`                                                                        | Meetings list (summaries; legacy command name)                                                                                                                                                                                                                                                                                                     | WP-68                 |
| `open_streaming_session(id)`                                                                       | Full Meeting (all decoded windows)                                                                                                                                                                                                                                                                                                                 | WP-68                 |
| `rename_streaming_session(id, title)` / `delete_streaming_session(id)`                             | Meeting library management, mirroring Transcription's                                                                                                                                                                                                                                                                                              | WP-68                 |
| `clear_streaming_session(id)`                                                                      | Clear a stopped Meeting's transcript, translations, MFU and accepted Prettify result while retaining its library row                                                                                                                                                                                                                               | WP-68                 |
| `set_streaming_translation_enabled(id, enabled)`                                                   | Persist the Live Translation switch's on/off state for a session, best-effort (WP-96 toggle pattern)                                                                                                                                                                                                                                               | WP-101                |
| `set_streaming_translation_target_language(id, target_language)`                                   | Persist the session-scoped `"en"`/`"ru"` target used to reopen the correct translation column                                                                                                                                                                                                                                                      | WP-115                |
| `start_streaming_session(engine?)`                                                                 | Starts the selected local ASR (Whisper or Qwen3-ASR) or the selected Cloud WebSocket provider; Cloud authenticates/connects before system-audio capture, persists final turns, and returns once capture starts (macOS only)                                                                                                                          | WP-106                |
| `stop_streaming_session()`                                                                         | Drop the held capture, cascading to end decode/persist and release the shared context (macOS only)                                                                                                                                                                                                                                                 | WP-68                 |
| `get_live_capture_snapshot()`                                                                      | Read the backend-owned live-capture phase, source, session, generation, revision, and error for renderer hydration                                                                                                                                                                                                                                 | WP-112                |
| `generate_streaming_mfu(id)`                                                                       | Generate structured MFU for a Meeting transcript and persist it (Craft MFU)                                                                                                                                                                                                                                                                        | WP-77                 |
| `generate_streaming_prettify(id)`                                                                  | Generate a cleaned-transcript candidate for review; not persisted until accepted                                                                                                                                                                                                                                                                   | WP-75                 |
| `accept_streaming_prettify(id, text)`                                                              | Persist an accepted prettify candidate                                                                                                                                                                                                                                                                                                             | WP-75                 |
| `revert_streaming_prettify(id)`                                                                    | Delete the accepted prettification, restoring the raw per-window transcript                                                                                                                                                                                                                                                                        | WP-75                 |
| `translate_streaming_window(session_id, window_index, target_language, text, context?)`            | Translate one Meeting window into `"en"`/`"ru"` via the active summary LLM and persist it, keyed by `(session_id, window_index, target_language)`; called by the Live Translation queue (WP-93/WP-103). Optional `context` is the up-to-2 immediately preceding windows' own translations, concatenated, passed as reference-only prompt context   | WP-92, WP-100, WP-103 |
| `list_streaming_translations(session_id, target_language)`                                         | Read every persisted translation for a session and target language, so the Live Translation queue (WP-93) reuses stored results instead of re-running the model                                                                                                                                                                                    | WP-93                 |
| `get_microphone_permission_status()` / `request_microphone_permission()`                           | Read TCC state without prompting, or explicitly request microphone access                                                                                                                                                                                                                                                                          | WP-109                |
| `list_recorder_sessions()` / `open_recorder_session(id)`                                           | Read Recorder history summaries or a complete session with persisted ASR provenance and segments                                                                                                                                                                                                                                                   | WP-109                |
| `create_recorder_draft()`                                                                          | Persist and return an audio-free Recorder draft immediately so it can be selected, renamed, or deleted before capture                                                                                                                                                                                                                              | WP-109                |
| `rename_recorder_session(id, title)` / `delete_recorder_session(id)`                               | Manage inactive Recorder history and its app-owned audio                                                                                                                                                                                                                                                                                           | WP-109                |
| `clear_recorder_recording(id)`                                                                     | Delete inactive Recorder audio, raw segments, and polish while retaining the named row as an empty draft; fail without clearing text when audio cleanup fails                                                                                                                                                                                      | WP-109                |
| `start_recorder_session(draft_id?)` / `stop_recorder_session()`                                    | Run preflight, activate the selected draft in place (or create a shortcut-started session), begin default-microphone capture, or enter durable finalization                                                                                                                                                                                        | WP-109                |
| `recover_recorder_session(id)` / `export_recorder_wav(id)`                                         | Reconcile recoverable local audio or export the finalized native-rate CAF as WAV                                                                                                                                                                                                                                                                   | WP-109                |
| `update_recorder_segment(session_id, segment_id, text)`                                            | Persist an edit without rewriting raw audio or ASR provenance                                                                                                                                                                                                                                                                                      | WP-109                |
| `set_recorder_shortcut(value)` / `get_recorder_shortcut_status()`                                  | Atomically replace the global toggle chord or report its registration state                                                                                                                                                                                                                                                                        | WP-109                |
| `generate_recorder_polish(id)` / `accept_recorder_polish(id, text)` / `revert_recorder_polish(id)` | Generate, accept, or discard a derived local-LLM presentation while preserving raw segments and audio                                                                                                                                                                                                                                              | WP-109                |

Events: `transcription_progress { id, percent }` reports Whisper's 0–100
Transcription decode estimate. `transcription_phase { id, phase: "diarizing" }` marks the transition
between the Transcription run's two passes; completion and errors return through the
`transcribe_meeting` invoke promise. `model_download_progress { id, fraction,
stage }` uses `stage` =
`downloading` while bytes arrive and `verifying` while the fetched file is
SHA-hashed, a pass long enough on a large model that the UI must name it rather
than show a full bar. `Segment` is
the shared transcript unit
(`{ id, start_ms, end_ms, text, speaker_id? }`). Errors are `AppError` serialized
to a human-readable string.

**Meeting events:** `streaming_window { session_id, window_index, start_ms,
end_ms, text, language, outcome_ok }` fires once per decoded window, whether
it succeeded or fail-open-skipped (`outcome_ok` distinguishes the two, same
convention as the persisted row). `streaming_sources { session_id, mic,
system_audio }` fires once, right after a session starts. The compatibility
payload is always `mic: false, system_audio: true` for supported Meeting
capture. `streaming_session_ended { session_id }` fires once the decode
loop has fully ended after `stop_streaming_session`.

**Recorder events:** `recorder_partial { session_id, revision, text }` replaces
the one provisional phrase; `recorder_segment_committed` and
`recorder_session_changed` hydrate durable transcript/session updates;
`recorder_error { session_id?, message }` reports actionable capture, decode,
or finalization failure. Recorder also emits the shared `live_capture_state`.
