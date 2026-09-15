# Local AI architecture

Part of the [architecture map](../architecture.md). Covers local MFU,
Prettify, Live Translation, and the optional Cloud Meeting adapter.

## Structured MFU (M3, `llm.rs`)

llama.cpp running the selected quantized local text model on Metal generates
**structured MFU** from a transcript: summary, key decisions, action items, open
questions, participants — in Russian or English depending on which the
transcript itself is in (Cyrillic-character detection in `llm::build_prompt`).
`llm::generate_mfu` returns a domain-agnostic `GeneratedNotes` (no id field);
each caller attaches its own id before persisting. For Transcription, generation is
manual (the **Create MFU** button, enabled only after transcription finishes)
and UI-blocking; the result is copyable, not separately editable or
clearable. Meeting reuses the same `generate_mfu` call (WP-77,
`generate_streaming_mfu`) for a live transcript, gated the
same way (enabled only once the session is stopped) and persisted in its own
`streaming_mfu` table (`streaming_store.rs`), parallel to but independent
of Transcription's legacy `MFU` table.

MFU, Prettify and Live Translation share one application-owned `LlmRuntime`
stored in Tauri's managed `AppState`. Its bounded eight-job scheduler runs one
llama.cpp context at a time, prioritizes interactive translations ahead of
queued MFU and Prettify work, and returns an explicit retryable error on
overload. Immutable model weights are cached by canonical asset path plus a
fingerprint containing size, modification time, catalog SHA identity, and a
bounded digest of the installed file's head and tail. The catalog asset is
fully SHA-verified before atomic installation; the bounded runtime digest also
detects a later same-size/same-mtime replacement without hashing a multi-GB
GGUF on every inference. Each job receives a fresh logical context. A single
runtime-owned idle worker resets its deadline after every completed or failed
job and drops the cached text-model weights after two minutes without LLM
work. It checks the scheduler under the same lock before eviction, so active
or queued inference is never unloaded; the shared llama.cpp backend stays
available for the next lazy load. Selecting or
deleting the active model enters a highest-priority mutation barrier after the
current inference, bumps the model generation, clears the cache, and cancels
jobs that were already queued against the old selection. Every prompt is
conservatively preflighted, then checked again with the
real tokenizer before decode. MFU output that is not valid structured JSON
fails explicitly instead of being presented as a summary-only success.
Application ownership also releases cached Metal resources before llama.cpp's
process-global teardown.

Commands resolve the selected model path only after their job owns the runtime
scheduler lease. A higher-priority model mutation therefore completes before a
queued old-generation job can observe a path, closing the settings/deletion
TOCTOU window as well as invalidating the cached weights.

## Transcript Prettify (WP-75, `llm.rs`, `src/diff.ts`) — Meeting and Recorder

A second, distinct local-LLM use of the same model: `llm::prettify_transcript`
performs conservative cleanup of a transcript, returning plain cleaned text
(not the structured-MFU JSON template `generate_mfu` uses). Its bilingual
prompt permits a lexical ASR correction only when the source is contextually
impossible and one phonetically close replacement is overwhelmingly clear.
Slang, conversational phrasing, jargon, names, brands, technical terms,
numbers, identifiers, and language switches remain protected instructions;
ambiguous fragments stay verbatim.
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
upsert/cascade-delete shape as `streaming_mfu`); the `Revert Prettify`
control calls `revert_streaming_prettify` to delete that row and restore the
raw per-window transcript for display/copy/export. The two LLM-generation
commands reuse `streaming::build_streaming_transcript`'s guards (session
exists, stopped, non-empty transcript); Accept and Revert operate on an
existing session row. The shared backend scheduler prevents Craft, Prettify
and Translation from executing concurrent llama.cpp contexts even if a caller
bypasses the UI's disabled controls.

## Live Translation (WP-92 core/persistence, WP-93 UI, WP-103 rolling per-window; `llm.rs`, `src/StreamingView.tsx`)

A third local-LLM use of the same shared model, alongside Craft MFU and
Prettify: `llm::translate_paragraph` translates one Meeting window's text
into a target language (`"en"` or `"ru"`) via the same llama.cpp completion
path — the function itself kept its original name and signature across
WP-103 (it never had a paragraph-specific concept; only its caller's unit
changed, see below) — using its own prompt (`build_translate_prompt`,
branching on the target language) and its own candidate validation
(`validate_translation_candidate`) — rejecting an empty or whitespace-only
result, a result disproportionately shorter or longer than the source, and a
result still predominantly in the source script when the target script
differs. Exact token retention applies to digit-bearing and underscore
identifiers plus ASCII acronyms and code-like hyphenated tokens. A natural
cross-script hyphenated term may be translated or transliterated (for example
`Навье-Стокса` to `Navier-Stokes`) instead of being falsely rejected as a
missing identifier. `ensure_translation_fits_context_budget` rejects (rather than
truncates) text whose estimated token count would overflow `CTX_SIZE`,
using a script-aware chars-per-token estimate — Cyrillic tokenizes denser
than Latin under Qwen/ChatML-style tokenizers, so the same character count
budgets fewer tokens for Cyrillic text.

Committed translations and Cloud provisional previews enter the shared bounded
LLM scheduler instead of racing through a separate busy flag. Committed
translation has priority over queued preview, MFU, and Prettify work. One active
llama.cpp context remains non-preemptive, but queued committed work overtakes
stale provisional work; capacity overload stays explicit and retryable.
Committed translation is capped at 256 generated tokens and a Cloud preview at
128, while MFU and Prettify retain the 1,024-token long-form budget.

The `translate_streaming_window(session_id, window_index, target_language,
text, context)` command (`commands/mfu.rs`, renamed from
`translate_streaming_paragraph` by ADR-016/WP-103) validates the request
cheaply first (`streaming::ensure_translation_request_is_valid`: supported
target language, session exists, non-empty source text), then runs
`streaming::translate_and_store_if_current` on
a `spawn_blocking` task. Before inference, the store verifies that the exact
source window is still current and successfully decoded; failed or stale
windows therefore consume no Metal time. The scheduler-admitted job resolves
the active model path. After inference, one conditional SQLite
`INSERT … SELECT` atomically verifies the enabled session toggle, a successful
decode, and the exact current source window while writing: disabling
translation, failing, or replacing the source while the model is running
cancels the write instead of persisting a stale translation. A
still-current result is upserted into `streaming_translations` (see Meeting
Persistence above) and returned. `context` (WP-100,
generalized by WP-103) is threaded unchanged through `translate_and_store`
into `llm::translate_paragraph`'s `prior_context` parameter; omitted or
`None`, the call and its resulting prompt are byte-identical to before
WP-100. `src/ipc.ts` exposes this as a typed `translateStreamingWindow`
wrapper with a `StreamingTranslationTargetLanguage` type and the matching
optional `context` parameter.

If a contextual result fails candidate validation, the backend retries that
window once without prior context. This recovers small-model context echo or
under-translation without weakening the persisted-result validator. The
`preview_streaming_translation` command uses the same prompt and validator at
lower scheduler priority but performs no database write.

Its read counterpart, `list_streaming_translations(session_id,
target_language)` (`commands/streaming.rs` → `streaming::
list_streaming_translations`), validates the target language and that the
session exists, then returns every persisted `{window_index, source_text,
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
rules. `groupWindowsIntoParagraphs` still drives this display grouping
exactly as before — WP-103 only changed what drives _translation_, not what
drives the on-screen row layout.

**WP-103 (rolling per-window translation, superseding WP-100's
paragraph-batch behavior — see ADR-016):** translation is triggered per
window, not per paragraph, and is entirely decoupled from paragraph
boundaries/`paragraphs.ts`'s closure heuristic (`isParagraphClosed` was
deleted as dead code once nothing gated on it). Window 0 starts translating as
soon as it is committed, without waiting for Stop or a second window. Every
window after that translates alone, as soon as it arrives, with `context` set
to the concatenation, in order, of the up-to-2 immediately preceding windows'
available (`"done"`/`"mirrored"`) translations — a failed or not-yet-resolved
predecessor is skipped rather than blocking, so translation always proceeds
with whatever context exists. Exactly one `translateStreamingWindow` call is
in flight at a time (unchanged single-flight queue, now keyed by
`window_index`), and the queue never delays rendering of incoming windows.
Before queuing anything, an effect calls `listStreamingTranslations` so a row
already persisted for this session and target language is reused without a
model call, matched by `window_index` against that one window's own current
text; a stale match (the window's text changed, e.g. a fail-open retry) is
re-queued instead. A window whose own language already matches the target is
mirrored individually — no model call, its own text used verbatim — which now
correctly handles a paragraph mixing already-translated and needs-translation
windows, rather than WP-93's original all-or-nothing paragraph-level check.
Turning the switch on backfills the whole session oldest-first through the
same per-window mechanism. The paired-row grid's translated cell is built by
mapping each of a paragraph's windows through its own entry (real text for a
done/mirrored window, a placeholder for one still in flight, and
`[unavailable]` for a failed ASR window) and
joining them, so a paragraph with an unfinished trailing window shows real
text for its finished windows and a placeholder only for the tail, instead of
staying blank until the whole paragraph resolves. The retry affordance stays
paragraph-scoped — one button per paragraph, re-enqueuing every _failed_
window within it, not every window. Switching the toggle off mid-queue, or
switching or deleting the session, cancels pending work and discards any
in-flight result a subsequent change has superseded, exactly as before
WP-103.

During Cloud capture the latest unstable ASR phrase is also translated as an
italic, non-persisted preview in the target column. Partial revisions are
coalesced to one active request plus the newest pending text; an arriving
committed window replaces the preview, and a late preview result is ignored.
Local Whisper capture deliberately waits for a committed window before using
the local text LLM: running Qwen for every unstable Whisper hypothesis contends
for Metal and can make audio ingestion fall behind. Its provisional target cell
therefore shows a stable finalization wait instead of starting inference. Every
queued item carries its session, target language, and cancellation generation,
so completion of an older inference cannot dequeue new-session work with a
stale target captured by a React render.

The switch's own on/off state and target language are persisted per session:
`streaming_sessions` owns `translation_enabled` and
`translation_target_language` (`"en"`/`"ru"`). They are written best-effort via
`set_streaming_translation_enabled` on every toggle (mirroring WP-96's
MFU-panel-toggle pattern — the switch keeps showing what the user chose even
if the write fails, with no retry) and
`set_streaming_translation_target_language` on target changes, then read back
into both `StreamingSessionSummaryDto` and `StreamingSessionDto`. Opening a session
restores both values, so stored English translations cannot be reopened under
the Russian UI default. The state survives closing and reopening a session and
an app restart. Pressing Start/Resume on the session that is
_already_ open is not a session-identity change, so it leaves the switch,
its translations, and its in-flight queue untouched — only starting a
genuinely different session (a brand-new one, or resuming a different past
session) resets their result data. A language choice made before a new capture
starts is preserved, because the active header cannot be changed once capture
has started.

### Cloud Meeting BYOK (`cloud_provider.rs`, `cloud_streaming.rs`) — WP-106

`cloud_provider.rs` defines the closed `CloudProvider` catalog and models:
Deepgram/Nova-3, AssemblyAI/Universal-3.5 Pro, and OpenAI/GPT Transcribe.
The selected identifier is the only Cloud field written to `settings.json`
(`cloud_provider`, default `deepgram`). `KeychainCredentialStore` uses the
macOS Keychain service `com.whisperpilot.cloud-api-keys`, with the provider id
as account. It reports only whether a key exists; keys never appear in DTOs,
settings, errors, logs, or frontend state after submission. The UI requires a
successful no-audio provider verification before enabling Save, and the save
command independently repeats that verification before writing Keychain. For
OpenAI, verification is an authenticated lookup of the selected
`gpt-transcribe` model only: it verifies model access without creating a
Realtime session or sending audio. The Realtime session configuration is
validated separately, immediately before capture begins.

The command facade returns a `CloudProviderConfiguration` with provider metadata
and configured booleans for get/select/save/remove, plus a `verify` command
which returns no credential material. `cloud_streaming.rs` owns provider-
neutral PCM conversion, documented WebSocket setup, transient partial events,
and final-turn parsing. OpenAI opens the dedicated Realtime transcription socket with
`intent=transcription`, then selects `gpt-transcribe` only inside the
transcription configuration in `session.update`; no voice/reasoning Realtime
model is inserted into this path. The configuration explicitly disables turn
detection so the app owns seven-second accuracy-oriented turn boundaries. The
connection is established before capture starts; OpenAI additionally waits for
the asynchronous
`session.updated` confirmation after its `session.update`. Capture samples are
then sent only to the selected provider. ScreenCaptureKit supplies OpenAI's
24 kHz mono samples natively, so the upload path performs no second resample.
OpenAI audio is committed in explicit seven-second turns, with remaining
non-empty audio committed and its completion
drained before shutdown, so each turn has more context before decoding and
final turns are persisted locally. OpenAI's incremental delta fragments are
accumulated per provider `item_id`; the IPC event keeps that id so an out-of-
order final clears only its matching transient partial rather than flashing
individual words or removing a newer turn. Failure emits a safe, stage-specific
retryable error, stops capture, and never falls back to Local.
If bounded capture or relay pressure creates the first capture-clock gap, the
transport drains already-enqueued provider results in FIFO order, emits one
failed span ending at the original capture position, closes the connection,
and moves live capture to the red error state. It does not continue a shifted
cloud timeline after lost audio.
