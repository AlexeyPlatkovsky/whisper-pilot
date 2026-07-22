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
  (a button each) and long-running. While one runs, the UI is blocked except the
  control that stops/tracks it, and progress is always shown — a bar with real
  progress, or a spinner with a live timer when real progress isn't available.
- **Edit in place, saved automatically.** Transcript segments and MFU text are
  edited where they are read and persist as you go — no save button, no lost work.
- **English UI, Russian transcription.** The app **UI language defaults to
  English** (more languages at release); the **transcription** default is
  **Russian** with auto-detect (ADR-007). The two are independent settings.
- **Configurable, not cluttered.** A single **Settings** screen holds models,
  appearance, and language; the workspace itself stays focused.

## Layout — the two-pane shell

```
┌───────────────────────────────────────────────────────────────────────┐
│ ◎◎◎  [⇤ panel]  [logo]                                  [⚙ settings]   │  ← header row 1 (traffic lights, fixed left controls, gear far right)
├───────────────┬───────────────────────────────────────────────────────┤
│  Meetings      │  ⟨Meeting title⟩  [edit][copy][delete]   [model ▾]     │  ← header row 2 (meeting header)
│  ───────────   │  [lang ▾]                     [Transcribe][Stop][MFU]  │
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

### Header controls (fixed, position never changes)

Row 1 — global, laid out left-to-right immediately after the macOS window
controls (close / minimize / zoom):

- **Panel toggle** — hide/show the left pane. Sits directly after the traffic
  lights; its position is fixed and it reflects toggled/untoggled state. The
  collapsed/expanded choice **persists** across restarts.
- **App logo** — directly after the toggle (VoicePilot logo for now). Fixed
  position; purely decorative.
- **Settings gear** — a fixed control at the **far right** of row 1; opens the
  **Settings** screen (models, appearance, app language; app update at release).

Row 2 — the active meeting's header:

- **Meeting label** with three action buttons: **edit** (opens the rename
  modal), **copy** (copies the full transcript to the clipboard), **delete**
  (with confirmation — same as the list action).
- **Model switcher** — dropdown of available Whisper models; default **large**.
  If **no model is available**, the switcher shows nothing and the status bar
  shows a warning.
- **Language selector** — Russian (default) / auto-detect; applies to the next
  Transcribe run.
- **Transcribe** — icon button with hover text. **Disabled** when no file is
  attached.
- **Stop transcribe** — icon button with hover text. **Disabled by default**;
  enabled only while a transcription is running.
- **Create MFU** — icon button with hover text. **Disabled** until a
  transcription has **finished** (mirrors the button inside the MFU section).

### Status bar (directly under the header)

Single line reflecting the meeting's current state:

| State | Shows |
| --- | --- |
| **Waiting for file** | prompt to attach a file; only relevant controls enabled |
| **File attached** | the attached file with an **×** button (delete, no confirmation). MVP: **one** file per meeting |
| **Transcribing** | progress **bar** across the transcribing → **identifying speakers** phases if real progress is available, else a **spinner + live timer** (updates every second). **All UI blocked except Stop** |
| **Finished** | the final transcription is ready; Create MFU becomes enabled |
| **Creating MFU** | spinner + live timer; **the whole UI is blocked** (no cancel for MFU) |
| **No model** | warning that no Whisper model is available |

### Center — transcript

- Fills the upper region of the right pane (≈70–85% height, complementing the
  MFU section below).
- **M2 (diarization included):** each speaker's turns render in a **colored
  bubble** — **10 predefined shades** cycled across speakers for the MVP —
  grouped and labelled by speaker (Спикер 1, Спикер 2, …).
- Each segment is an auto-sizing editable field prefixed by its timestamp
  (`m:ss`); edits auto-save. Bubble grouping/coloring is F002; the editable
  segment surface is F004.

### MFU section (bottom of the right pane)

- **Empty (default):** occupies **15%** of the center height. Shows a **Create
  MFU** button plus a small **"Create MFU"** label beneath it (the same action as
  the header button; disabled until transcription has finished).
- **Populated:** occupies **30%** of the center height. Shows the MFU **text**
  only, with three icon actions (hover text) in the **top-right corner**:
  **edit**, **copy**, **clear**.
- MFU text is editable in place and auto-saves; **copy** places it on the
  clipboard; **clear** empties the section (returns to the 15% empty state).

### Settings (F005)

Opened from the header **gear**; a screen with these sections:

- **AI models** — grouped by task (transcription, diarization; notes at release).
  Each required model shows its state with **Download** and **Delete** buttons.
  **Download** opens a blocking dialog (progress bar, spinner, elapsed time),
  the same "blocked with progress shown" pattern as transcription/MFU; it closes
  itself once the model is verified (SHA) and ready, or on error, or if the user
  dismisses it early with **✕** (the download itself keeps running and the model
  still updates to ready in the background). **Delete** asks for confirmation
  before removing the file. Beta lists **one model per task**; at release each
  task may list 3–4 models, each with an **Active** radio.
- **Appearance** — theme choice: **Light / Dark / System** (System follows the
  OS). At release, 3–4 extra named themes, each in a light and dark variant.
- **App language** — the **UI** language; **English** by default (only option in
  beta). At release: Russian, Turkish, Spanish, German, French. Independent of the
  transcription language.
- **Update app** — *release only*: check for and apply application updates.

Changes apply immediately and persist. A model that is not downloaded is flagged
here and disables/degrades its task in the workspace (Transcribe needs the
Whisper model; diarization degrades without its models).

## User Flows

### Adjust settings (F005)

1. Click the header **gear** → the Settings screen opens.
2. **AI models:** Download (blocking progress dialog + SHA verify) or Delete
   (confirm first) each task's model.
3. **Appearance:** pick Light / Dark / System — applies at once.
4. **App language:** English (beta). All choices persist across restarts.

### Create & transcribe a meeting (M2)

1. Click **+ New meeting** → an empty meeting is created and selected.
2. Attach an audio/video file → it appears in the status bar with an **×**. The
   meeting's title defaults to the file name (renamable any time).
3. (Optional) pick the **model** and **language**.
4. Press **Transcribe** → the UI blocks (except **Stop**); the status bar shows a
   progress bar (or spinner+timer) across two phases: **transcribing**, then
   **identifying speakers** (diarization runs automatically — there is no separate
   diarize action).
5. On completion the transcript appears in the center as **colored per-speaker
   bubbles**; status bar shows **Finished**; **Create MFU** enables. Everything
   auto-saves to the library.
   - **Stop** during a run cancels it (transcription and diarization together); no
     partial transcript is kept.
   - If diarization is unavailable, the transcript still appears as plain segments
     with a note; the run does not fail.
   - Pressing **Transcribe** again on a meeting that already has a transcript
     **warns** it will replace the transcript and any MFU, and proceeds only on
     confirmation.

### Reopen / manage a meeting (M2)

1. The left list holds every meeting; click one to open it.
2. **Rename** (modal, ≤120 chars, non-empty) or **delete** (confirmation) from
   the list row or the header.
3. If the source file is missing, the meeting still opens for reading/editing;
   **Transcribe** is disabled with a "source file missing" note.

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

1. From a meeting, **export** → Markdown or plain text; pick a destination. The
   header **copy** action copies the transcript to the clipboard.

## States

- **No meeting selected** — right pane empty with a prompt; only **+ New meeting**
  is meaningful.
- **Empty meeting, waiting for file** — status bar prompts to attach; Transcribe
  disabled.
- **File attached** — status bar lists the file with ×; Transcribe enabled.
- **Transcribing** — UI blocked except Stop; progress bar or spinner+timer.
- **Transcription error** — a banner with the `AppError` message (ffmpeg missing,
  model missing); the meeting is otherwise unchanged.
- **Finished** — transcript populated; Create MFU enabled.
- **Creating MFU** — whole UI blocked; spinner+timer; no cancel.
- **MFU populated** — MFU section at 30% with edit/copy/clear.
- **Source file missing** — meeting opens; Transcribe disabled with an
  explanatory note; transcript/MFU remain editable.
- **No model available** — model switcher hidden; status-bar warning; the
  Settings → AI models section flags the missing model with a Download action.
- **Settings open** — the Settings screen (models / appearance / app language);
  changes apply immediately and persist.
- **Model downloading** — a model shows download progress; on SHA-verified
  completion it becomes ready and its task is enabled; Delete returns it to
  not-downloaded.

## Interaction Patterns

- **Segment editing** — each segment is an auto-sizing field prefixed by its
  timestamp (`m:ss`); edits auto-save.
- **Speaker bubbles (M2)** — 10 predefined shades cycled across speakers; a
  speaker's inline rename applies everywhere for that speaker.
- **MFU section** — edit in place (auto-saved); copy to clipboard; clear resets
  to empty. Generation is manual and UI-blocking.
- **Meeting rename** — modal, ≤120 chars, non-empty, Save/Cancel.
- **Deletes** — both the list-row delete and the header delete confirm first.
- **Blocking feedback** — a running Transcribe or Create MFU blocks the UI (only
  Stop stays live during Transcribe); progress/spinner is always visible;
  failures are shown, not swallowed.

## Accessibility

- Full keyboard operability; editable regions are standard fields in reading
  order; the rename modal traps focus and closes on Escape (= Cancel).
- Icon buttons carry hover text and accessible labels (panel toggle, transcribe,
  stop, create MFU, edit/copy/delete/clear).
- Adequate contrast in light and dark schemes (follows the OS color scheme).
- Speaker distinction (M2) never relies on color alone — each bubble carries the
  speaker name as text.
