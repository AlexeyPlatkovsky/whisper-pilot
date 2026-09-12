# Design

Owns the product/UX design: the workspace shell, screens, flows, and states.
Technical structure is in `architecture.md`. UX flows and view states live here;
the **visual language and design tokens** (colours, spacing, radii, typography)
live in the design book `designbook.md`, whose source of truth is
[`../src/tokens.css`](../src/tokens.css) (derived from `pencil/main_view.pen`).
The unit of work is a **Meeting** (one transcription of one source file; see
`glossary.md`).

## UX Principles

- **Workspace, not a wizard.** The app is one persistent two-pane shell: a
  **Meetings** list on the left, the active meeting's workspace on the right.
  Past work is always in reach; nothing is modal except confirmations and rename.
- **One meeting in focus.** The right pane always shows exactly one meeting:
  header, status bar, transcript, then MFU beneath it.
- **Explicit, honest actions.** Transcription and MFU generation are **manual**
  (a button each) and long-running. While one runs, the UI is blocked and an
  indeterminate spinner with a live timer shows that work is continuing.
- **Edit in place, saved automatically.** Transcript segments and MFU text are
  edited where they are read and persist as you go — no save button, no lost work.
- **English UI, detected transcription language.** The app **UI language defaults
  to English** (more languages at release). The **transcription** language is
  never chosen — Whisper detects it from the audio on every run (ADR-012). The
  UI language is a setting; the transcription language is a result.
- **Configurable, not cluttered.** A single **Settings** screen holds models,
  appearance, and the **app** language; the workspace itself stays focused.

## Layout — the two-pane shell

```
┌───────────────────────────────────────────────────────────────────────┐
│ ◎◎◎  [⇤ panel]  [logo]                                  [⚙ settings]   │  ← header row 1 (traffic lights, fixed left controls, gear far right)
├───────────────┬───────────────────────────────────────────────────────┤
│  Meetings      │  ⟨Meeting title⟩  [edit][copy][delete]   [model ▾]     │  ← header row 2 (meeting header)
│  ───────────   │                                     [Transcribe][MFU]  │
│  [+ New]       ├───────────────────────────────────────────────────────┤
│                │  status bar: waiting / files / transcribing / done …   │  ← status bar
│  • Meeting A   ├───────────────────────────────────────────────────────┤
│  • Meeting B   │                                                        │
│  • Meeting C   │   transcript — one colored bubble per speaker (M2)     │  ← center (≈70–85% height)
│    …           │                                                        │
│                │                                                        │
│                ├───────────────────────────────────────────────────────┤
│                │   MFU section (15% empty · 30% when populated)         │  ← MFU (bottom of center)
└───────────────┴───────────────────────────────────────────────────────┘
```

### Left pane — Meetings list

- Mirrors VoicePilot's Sessions list.
- **`+ New meeting`** at the top creates an empty meeting and selects it (the
  entry point for a new transcription — you then attach a file and press
  Transcribe).
- Each row shows the meeting title (and secondary meta: source name / date).
  Click to open in the right pane.
- Per-row actions: **rename** and **delete**.
  - **Rename** — a modal with an input, **Save** and **Cancel**. Input max **120
    characters**; empty values are rejected (Save disabled). Pre-filled with the
    current title.
  - **Delete** — with a **confirmation** dialog.

### Workspace mode control

The top of the left pane contains a three-way **Meeting / Streaming / Recorder**
control. Changing the visible workspace never starts or stops native work. If
Meeting transcription, Streaming capture, or Recorder capture/finalization owns
the shared transcription resource, incompatible Start actions remain disabled
in every workspace until that owner releases it.

Recorder replaces the list beneath the control with saved voice-note sessions
and a **New recording** action. Each row shows title, date or live state, and
duration when known. Selecting another mode does not discard the active Recorder
session or its visible transcript.

### Header controls (fixed, position never changes)

Row 1 — global, laid out left-to-right immediately after the macOS window
controls (close / minimize / zoom):

- **Panel toggle** — hide/show the left pane. Sits directly after the traffic
  lights; its position is fixed and it reflects toggled/untoggled state. The
  pane **defaults open on every launch**; the collapsed/expanded choice does
  not persist across restarts.
- **App logo** — directly after the toggle (VoicePilot logo for now). Fixed
  position; purely decorative.
- **Settings gear** — a fixed control at the **far right** of row 1; opens the
  **Settings** screen (models, appearance, app language; app update at release).

Row 2 — the active meeting's header:

- **Meeting label** with three action buttons: **edit** (opens the rename
  modal), **copy** (copies the full transcript to the clipboard and confirms
  with a brief checked button state and "Copied!" toast), **delete**
  (with confirmation — same as the list action).
- There is no in-header model switcher; model selection lives in
  Settings → AI models only. If **no model is available**, the status bar
  shows a warning.
- **Transcribe** — icon button with hover text. **Disabled** when no file is
  attached. Meeting transcription runs to completion; safe cancellation is
  deferred to the isolated-worker design tracked by WP-87.
- **Create MFU** — icon button with hover text. **Disabled** until a
  transcription has **finished** (mirrors the button inside the MFU section).

### Status bar (directly under the header)

Single line reflecting the meeting's current state:

| State                | Shows                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Waiting for file** | prompt to attach a file; only relevant controls enabled                                                                                                                                                                                                                                                                                                                                                                                      |
| **File attached**    | the attached file with an **×** button (delete, no confirmation). MVP: **one** file per meeting                                                                                                                                                                                                                                                                                                                                              |
| **Transcribing**     | indeterminate spinner + live timer (updates every second). When the run moves into **identifying speakers**, the persisted transcript appears read-only while the spinner+timer remain. Actions stay blocked until the complete run returns. |
| **Finished**         | the final transcription is ready; Create MFU becomes enabled                                                                                                                                                                                                                                                                                                                                                                                 |
| **Creating MFU**     | spinner + live timer; **the whole UI is blocked** (no cancel for MFU)                                                                                                                                                                                                                                                                                                                                                                        |
| **No model**         | warning that no Whisper model is available                                                                                                                                                                                                                                                                                                                                                                                                   |

### Center — transcript

- Fills the upper region of the right pane (≈70–85% height, complementing the
  MFU section below).
- **M2 (diarization included):** each speaker's turns render in a **colored
  bubble** — **10 predefined shades** cycled across speakers for the MVP —
  grouped and labelled by speaker (Спикер 1, Спикер 2, …).
- Each segment is an auto-sizing editable field prefixed by its timestamp
  (`m:ss`); edits auto-save. Bubble grouping/coloring is F002; the editable
  segment surface is F004.
- The transcript panel's own header row (title + segment count, right-aligned
  actions) carries a **Diarize** icon button next to the Editable indicator —
  re-runs speaker identification alone on the current transcript, without
  re-transcribing. Disabled until a transcript exists, a diarization model is
  active, and the source file is still readable; disabled together with every
  other run-blocking control while Transcribe, Craft MFU, or Diarize itself
  is in flight. Mirrors Streaming's Prettify button position in its own
  transcript header.
- **Streaming only:** the one-row transcript header has the title followed by
  `AI` and icon-only **Local / Cloud** controls (Local is the default). Choosing
  Cloud before capture immediately shows the status-row disclosure “Cloud
  transcription sends live audio to <provider>. Usage is billed to your
  account.” on every Cloud selection. A Cloud start connects the selected
  provider before capture; connection failure is shown as a retryable error and
  never starts Local capture as a fallback. The
  middle slot uses a **languages icon**, a switch (`role="switch"`, WP-93),
  and a target-language dropdown (English / Русский, defaulting to Russian on
  every launch — the choice is not persisted); the words “Live Translation”
  are not rendered. The dropdown locks while the switch is on; to change the
  target, switch off, pick, then switch back on. The switch is disabled with a
  stated reason when no LLM model is ready, when a prettified transcript is
  showing, or while a Prettify review is pending — and the reverse also holds:
  **Prettify** is disabled with a stated reason while Live Translation is on.
  While live capture is active, the whole transcript header is visually muted
  and every action element, including engine, language, and MFU controls, is
  unavailable. Meeting's header has none of these Streaming-only controls.
- **Live Translation on** replaces Streaming's transcript flow with a
  two-column paired-row view inside the same scroll region: a header row
  names the source side ("Original · auto-detected") and the target
  language, then one row per paragraph — left cell the original text, right
  cell its translation — each carrying its own timestamp/language tag so a
  pair never drifts out of alignment. Translation itself runs per window,
  not per paragraph (WP-103), so the right cell is built by joining each of
  the paragraph's windows through its own state: real translated text once a
  window is done, the original mirrored in a muted style for a window
  already entirely in the target language (no model call happens), a
  "Translating…" spinner or "Pending…" for a window still in flight, or a
  failed marker for one that errored. A paragraph whose trailing window is
  still translating therefore shows real text for its finished windows and a
  live indicator only on the unfinished tail, rather than the whole cell
  staying blank until every window in it resolves. The retry control stays
  one per row (paragraph), re-running every failed window within it rather
  than one control per window. Switching the toggle off restores the
  single-column view unchanged. Below the app's minimum supported window
  width, the compact icon controls and one-row header stay available without a
  segment-count caption, so it does not overflow with the MFU panel open.
- At the **right end** of that same actions cluster, a labelled **MFU**
  switch (`role="switch"`, WP-96) shows or hides the MFU section. Defaults
  **on**; the choice persists independently per screen (Meeting, Streaming)
  and survives restart. It never gates Craft MFU, but is unavailable with the
  rest of the Streaming header during live capture. Running **Create
  MFU**/**Craft** while the panel is hidden reveals it automatically. Present
  identically on the Meeting and Streaming transcript headers while idle.

### Recorder workspace

Recorder uses the same shell, left library, and fixed top action vocabulary as
Meeting and Streaming. The left column provides mode switching, search, open,
new recording, rename, and confirmed deletion. The header keeps sidebar, new,
Settings, title, status, **Start**, **Stop**, Prettify, Copy, Export, and
transcript-only Clear controls in the same positions as the other workspaces.
**Start** performs
permission, model, system-default microphone, and caption-surface preflight
before a session is created. **Stop** ends capture and enters **Finalizing**;
copy, export, delete, and playback become available when their durable inputs
exist. The metadata row shows the default microphone and recording duration.
Recorder uses the same selected ASR model as Meeting and local Streaming;
Whisper is the default and Qwen3-ASR 1.7B Q8_0 is the alternative. Recorder
persists capture-window spans but does not currently render them. An unsupported
or missing selection blocks Start with a Settings action; it never silently
starts another engine.

Committed phrases have stable identity and normal text styling. Their capture
window spans are persisted but are not currently rendered in the Recorder
workspace. Consecutive live hypotheses promote their shared word prefix to
normal text; only the remaining replaceable suffix is italic at 80% opacity.
The partial is replaced rather than duplicated when its committed phrase
arrives.

Recorder retains app-owned CAF audio independently of transcript edits. The
bottom audio surface is disabled during capture, reports safe closing during
Finalizing, and offers playback plus **Export WAV** after completion. An
interrupted `.caf.partial` remains visible with **Recover** and **Delete**. A
failed delete remains visible as **Delete failed / Retry**. See ADR-017 for
ownership and atomic-finalization rules.

**Prettify transcript** sends the raw committed text to the selected local LLM,
persists the result, and replaces the visible raw text in the same transcript
panel. **Restore original transcript** returns to the editable raw segments.
**Clear transcript** requires confirmation, removes both raw and prettified
text, and retains the session and captured audio. Prettify and restore never
rewrite captured audio or raw segment rows.

The configurable global shortcut uses **Control + Option + Space** initially and
toggles Recorder Start/Stop from another application without activating the main
window. A replacement chord is registered before it replaces the persisted
working chord; failure keeps the previous chord, while first-registration failure
leaves the feature disabled with an explanation. Key repeats collapse to one
transition. A shortcut is ignored during Finalizing and cannot start Recorder
while another live source owns capture.

Shortcut-started recording creates a compact, non-activating live-caption
surface showing icon plus text for Ready, Recording, partial Listening,
Finalizing, and Error. Clicking it opens Recorder. If the surface disappears
after hidden capture starts, Recorder stops and finalizes instead of continuing
an invisible recording, and the main workspace shows the actionable error.

Clicking the app logo collapses main into a separate 120 px circular bubble at
the main window's former top-left. Idle is a yellow ring with a pause badge,
live microphone or system-audio capture is green with a microphone badge, and
a persistent actionable capture/ASR error is red with an alert badge. The
accessible name states the same status without relying on color. Pointer motion
through five logical pixels remains a click; motion beyond it starts native
drag and suppresses restore. Click or Enter/Space restores and focuses main at
the bubble top-left, clamped onto a visible monitor. Collapse shows the bubble
before main hides, and restore does the inverse so one recovery surface remains
visible on failure.

The **Over All** setting controls Always on Top and visibility across Spaces.
It applies immediately and persists. With it off the bubble behaves as a normal
floating app window; with it on it follows the user across workspaces and stays
above ordinary/fullscreen content where macOS permits auxiliary windows. Drag,
Retina scaling, Stage Manager, sleep/wake, and monitor removal do not start or
stop backend capture. The direct-DMG build uses transparency; an App Store build
would retain the interaction in an opaque 120 px panel.

### MFU section (bottom of the right pane)

- **Empty (default):** occupies **15%** of the center height. Shows a **Create
  MFU** button plus a small **"Create MFU"** label beneath it (the same action as
  the header button; disabled until transcription has finished).
- **Populated:** occupies **30%** of the center height. Shows the MFU **text**
  only, with three icon actions (hover text) in the **top-right corner**:
  **edit**, **copy**, **clear**.
- MFU text is editable in place and auto-saves; **copy** places it on the
  clipboard; **clear** empties the section (returns to the 15% empty state).
- **Hidden:** the header **MFU** switch (see Center — transcript, WP-96) can
  hide this section entirely regardless of empty/populated state, freeing its
  space for the transcript panel. Shown by default; the choice persists per
  screen. Running **Create MFU**/**Craft** while hidden reveals the section.

### Settings (F005)

Opened from the header **gear**; a screen with these sections:

- **AI models** — grouped by task (transcription, diarization, and MFU).
  Each required model shows its state with **Download** and **Delete** buttons.
  **Download** opens a blocking dialog (progress bar, spinner, elapsed time),
  the same "blocked with progress shown" pattern as transcription/MFU; it names
  its stage — _Downloading…_ while bytes arrive, _Verifying…_ while the fetched
  file is SHA-checked — and closes itself once the model is verified and ready,
  or on error, or if the user dismisses it early with **✕** (the download itself
  keeps running, and the model's row keeps reporting percent-complete and then
  _Verifying…_ until it updates to ready). **Delete** asks for confirmation
  before removing the file. Tasks may expose one or more fixed catalog entries;
  selectable entries use an **Active** radio. Transcription has two rows:
  Whisper and Qwen3-ASR 1.7B Q8_0. Each row exposes one selection used by all
  three modes. Model rows show the name, download size/status, and actions
  without language, timestamp, memory, or license guidance copy.
- **Appearance** — theme choice: **Light / Dark / System** (System follows the
  OS), and **Status Colors**: one configurable color per current semantic
  Meeting/Streaming status (anchored picker popover, per-row revert to the
  built-in default, Reset all behind a confirmation). Statuses are alphabetized
  and fill the two-column list left-to-right; a low-contrast picker warning
  compares the half-up rounded two-decimal ratio against `4.50:1`. At release,
  3–4 extra named themes, each in a light and dark variant.
- **Floating bubble** — an **Over All** checkbox for Always on Top/all-Spaces
  behavior. It applies immediately; the bubble position persists separately and
  is corrected if its monitor disappears.
- **App language** — the **UI** language; **English** by default (only option in
  beta). At release: Russian, Turkish, Spanish, German, French. Independent of the
  transcription language.
- **Cloud provider** — select one hardcoded provider/model row, styled like
  the local-AI model rows: **Deepgram — Nova-3**, **AssemblyAI — Universal-3.5
  Pro**, or **OpenAI — GPT Transcribe**. Each row exposes only whether a
  key is configured — a green check with **API Key** — and an icon-only
  **Manage API key** action. The compact in-app sheet uses icon-only
  remove/verify/save actions (with accessible labels), a masked field, and a
  required **Verify API key** action before Save
  becomes available; verification contacts the selected provider without
  saving the key or sending captured audio. The backend verifies again before
  it writes to macOS Keychain. Replace and Remove remain available for an
  existing key; values are never displayed. The selected provider is used for
  the next new Cloud Streaming session. Provider selection and key management
  are disabled while Streaming capture is live.
- **Update app** — _release only_: check for and apply application updates.

Changes apply immediately and persist. A model that is not downloaded is flagged
here and disables/degrades its task in the workspace (Transcribe needs the
Whisper model; diarization degrades without its models).

## User Flows

### Adjust settings (F005)

1. Click the header **gear** → the Settings screen opens.
2. **AI models:** Download (blocking progress dialog + SHA verify) or Delete
   (confirm first) each task's model.
3. **Appearance:** pick Light / Dark / System — applies at once.
4. **App language:** English (beta).
5. **Cloud provider:** select a provider and use **Manage API key** to add or
   replace a masked Keychain credential. Verify it successfully before Save
   is enabled; remove an existing credential if needed. All non-secret choices
   persist across restarts.

### Create & transcribe a meeting (M2)

1. Click **+ New meeting** → an empty meeting is created and selected.
2. Attach an audio/video file → it appears in the status bar with an **×**. The
   meeting's title defaults to the file name (renamable any time).
3. (Optional) pick the **model**. The language is detected from the audio — there
   is nothing to choose (ADR-012).
4. Press **Transcribe** → the status bar shows an indeterminate spinner+timer
   across two phases: **transcribing**, then **identifying speakers** (diarization
   runs automatically — there is no separate diarize action). When speaker
   identification begins, the persisted transcript appears read-only; action
   controls stay blocked.
5. On completion the transcript updates to **colored per-speaker
   bubbles**; status bar shows **Finished**; **Create MFU** enables. Everything
   auto-saves to the library.
   - Meeting transcription currently runs to completion. Safe cancellation is
     tracked separately in WP-87 as an isolated-worker process.
   - If diarization is unavailable, the transcript still appears as plain segments
     with a detail; the run does not fail.
   - Pressing **Transcribe** again on a meeting that already has a transcript
     replaces the transcript and any MFU immediately, with no confirmation
     guard.

### Reopen / manage a meeting (M2)

1. The left list holds every meeting; click one to open it.
2. **Rename** (modal, ≤120 chars, non-empty) or **delete** (confirmation) from
   the list row or the header.
3. If the source file is missing, the meeting still opens for reading/editing;
   **Transcribe** is disabled with a "source file missing" detail.

### Record a voice note

1. Open **Recorder** or invoke its global shortcut while no live source owns the
   transcription resource.
2. On first explicit Start, macOS requests microphone access. Denial creates no
   history row and the UI links to the relevant System Settings pane.
3. After preflight, one session begins on the current system-default microphone.
   Committed phrases accumulate while one replaceable partial shows trailing
   Russian, English, or mixed speech. Audio checkpoints limit unflushed capture
   to one second.
4. Press **Stop** or invoke the shortcut again. The UI shows **Finalizing** until
   trailing speech and the CAF both reach their durable boundary.
5. Reopen the saved session to edit, copy, or export text, play its retained
   audio, or export a separate WAV copy. Deleting the session removes app-owned
   text and audio after confirmation but never removes an exported file.

### Review by speaker (M2)

1. Segments render grouped into per-speaker colored bubbles (Спикер 1, Спикер 2,
   …), cycling the 10 shades.
2. Renaming a speaker updates the label everywhere and auto-saves. (Reassigning a
   misattributed segment is not yet supported — documented limitation.)

### Create MFU (M3)

1. After **Finished**, press **Create MFU** (header or MFU section).
2. The whole UI blocks; the status bar shows a spinner + live timer.
3. On completion the MFU text renders in the section (grows to 30%), editable,
   copyable, clearable.

### Export (M2)

1. Settings → Export sets the file type (Markdown or plain text) once; **Save**
   then writes the transcript (and, for Markdown, the MFU when present) to a
   chosen destination in that format, and the header **copy** action copies
   the same rendering to the clipboard (WP-15). Plain text stays
   transcript-only, matching the pre-WP-15 rendering; Markdown adds `#`/`##`
   headers, bold speaker labels, and `[m:ss]` timestamps.

## States

- **No meeting selected** — right pane empty with a prompt; only **+ New meeting**
  is meaningful.
- **Empty meeting, waiting for file** — status bar prompts to attach; Transcribe
  disabled.
- **File attached** — status bar lists the file with ×; Transcribe enabled.
- **Transcribing** — UI blocked; indeterminate spinner+timer.
- **Transcription error** — a banner with the `AppError` message (ffmpeg missing,
  model missing); the meeting is otherwise unchanged.
- **Finished** — transcript populated; Create MFU enabled.
- **Creating MFU** — whole UI blocked; spinner+timer; no cancel.
- **MFU populated** — MFU section at 30% with edit/copy/clear.
- **Source file missing** — meeting opens; Transcribe disabled with an
  explanatory detail; transcript/MFU remain editable.
- **No model available** — status-bar warning; the Settings → AI models
  section flags the missing model with a Download action.
- **Settings open** — the Settings screen (models / appearance / app language);
  changes apply immediately and persist.
- **Model downloading** — a model shows download progress; on SHA-verified
  completion it becomes ready and its task is enabled; Delete returns it to
  not-downloaded.
- **Recorder ready** — no session or audio exists; Start and the configured
  shortcut are available when the shared transcription resource is idle.
- **Recorder permission required** — no session was created; the recovery action
  opens macOS System Settings and Start retries preflight.
- **Recorder recording** — microphone capture owns the live resource; status and
  duration are explicit and Stop is available.
- **Recorder partial text** — committed phrases remain stable while one styled
  trailing partial is revised in place.
- **Recorder finalizing** — Start and shortcut transitions are unavailable while
  trailing transcription, atomic audio finalization, and the full-audio quality
  pass retain resource ownership. A failed quality pass keeps the live text and
  finalized audio rather than converting a usable recording into an error.
- **Recorder completed** — transcript editing, playback, text export, and WAV
  export are available.
- **Recorder recoverable error** — durable transcript/audio remain visible with
  the specific recovery action; device loss never leaves a false live state.
- **Recorder delete failed** — the session stays visible with Retry rather than
  disappearing behind an unobservable cleanup operation.
- **Compact live caption** — a non-activating surface communicates Ready,
  Recording/partial, Finalizing, or Error with icon and text, never color alone.

## Interaction Patterns

- **Segment editing** — each segment is an auto-sizing field prefixed by its
  timestamp (`m:ss`); edits auto-save.
- **Speaker bubbles (M2)** — 10 predefined shades cycled across speakers; a
  speaker's inline rename applies everywhere for that speaker.
- **MFU section** — edit in place (auto-saved); copy to clipboard; clear resets
  to empty. Generation is manual and UI-blocking.
- **Meeting rename** — modal, ≤120 chars, non-empty, Save/Cancel.
- **Deletes** — both the list-row delete and the header delete confirm first.
- **Blocking feedback** — a running Transcribe or Create MFU blocks the UI; a
  spinner and live timer stay visible; failures are shown, not swallowed.

## Accessibility

- Full keyboard operability; editable regions are standard fields in reading
  order; the rename modal traps focus and closes on Escape (= Cancel).
- Icon buttons carry hover text and accessible labels (panel toggle, transcribe,
  create MFU, edit/copy/delete/clear).
- Adequate contrast in light and dark schemes (follows the OS color scheme).
- Speaker distinction (M2) never relies on color alone — each bubble carries the
  speaker name as text.
