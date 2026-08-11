# F004 Library & Workspace — Requirements

## Summary

Turn the stateless M1 transcription flow into a persisted, **two-pane workspace**:
a left **Meetings** list and a right meeting workspace (header, status bar,
transcript, MFU section). Meetings persist in a local SQLite library with
reopen/rename/delete and **auto-saved** edits. Transcription is a **manual**,
UI-blocking action with an indeterminate running status, with the language auto-detected.
Includes a model switcher, a source-missing state, and Markdown/plain-text
export. One file
per meeting. Detailed layout is owned by `design.md`; this feature specifies the
behavior.

## Serves

- `idea.md` scope: "a persisted library of meetings … edits auto-saved";
  "export … to Markdown and plain text"; "the transcription language … always
  auto-detected from the audio, never chosen".
- `roadmap.md` phase: **M2 — Library & workspace**.
- `design.md`: the two-pane shell, header, status bar, MFU section.
- ADR-008 (persistence), ADR-010 (shell + manual triggers).
- TaskPilot epic **WP-11**.

## Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| F004-R1 | The system shall persist each transcription as a **meeting** (title, source path/name, created date, duration, language, status) in a local SQLite library; a new meeting's title defaults to the source file name and is user-renamable. | must |
| F004-R2 | The system shall present a persistent **two-pane shell**: a left Meetings list and a right workspace for the selected meeting; a header **toggle** (placed after the macOS window controls, followed by the app logo) shows/hides the left pane, and the collapsed state persists across restarts. | must |
| F004-R3 | The system shall let the user create a new meeting (**+ New meeting**), and open, rename, and delete meetings from the list; rename is a modal (input ≤120 chars, non-empty, Save/Cancel) and delete requires confirmation. | must |
| F004-R4 | The system shall show a meeting header with the meeting label and **edit** (rename), **copy** (copies the full transcript to the clipboard), and **delete** (confirmed) actions. | must |
| F004-R5 | The system shall provide a **model switcher** (default the `large` model); when no model is available it shall hide the switcher and show a warning in the status bar. | must |
| F004-R6 | The system shall **auto-detect** the transcription **language** on every run and shall not offer the user any language choice; the detected code is stored on the meeting (ADR-012). | must |
| F004-R7 | The system shall let the user **attach one** source file to a meeting, shown in the status bar with an **×** to detach (no confirmation); transcription does not start on attach. | must |
| F004-R8 | The system shall transcribe only on the **Transcribe** action (disabled with no file attached); while a run is active the UI is blocked and the Meeting run proceeds to completion. The run includes diarization (F002); the meeting is **finished** only once transcription and speaker attribution are both done. Safe cancellation is deferred to WP-87's isolated worker. | must |
| F004-R9 | The system shall reflect state in a **status bar**: waiting-for-file, attached-file(s), transcribing (indeterminate spinner + live 1-second timer), identifying speakers (the same spinner + timer), finished, creating-MFU (UI-blocked spinner + timer), and no-model warning. | must |
| F004-R10 | The system shall auto-save edits (segment text, and later speaker labels and notes) to the library without an explicit save action. | must |
| F004-R11 | The system shall provide the editable transcript surface in the center pane (auto-sizing `m:ss` segments); per-speaker **colored bubble** grouping/coloring is rendered by F002 (also M2). | must |
| F004-R12 | The system shall open a meeting whose source file is missing, disable re-transcribe, and show an explanatory note. | must |
| F004-R13 | The system shall export a meeting to Markdown or plain text, including per-speaker labels and timestamps in the transcript and the MFU notes when present. | must |
| F004-R14 | Re-running **Transcribe** on a meeting that already has a transcript shall warn that the existing transcript and any MFU will be replaced, and proceed only after confirmation. | should |

## Acceptance Criteria

- **F004-R1:** after transcription, a meeting with correct metadata appears in the
  list and survives app restart; its title defaults to the source file name.
- **F004-R2:** the left pane toggles from the header button; its position never
  moves; the collapsed/expanded state is restored after restart.
- **F004-R3:** + New meeting adds and selects an empty meeting; rename rejects
  empty and >120-char input; delete asks first and removes from list and DB.
- **F004-R4:** the header copy action places the full transcript on the clipboard;
  edit opens the rename modal; delete confirms.
- **F004-R5:** the switcher lists available models with `large` default; with none
  installed it is hidden and the status bar warns.
- **F004-R6:** no language control is presented anywhere; an English recording
  transcribes as English and the meeting records the detected code.
- **F004-R7:** attaching shows the file with an ×; × detaches it; a second file
  replaces the first (one at a time); attach alone starts nothing.
- **F004-R8:** Transcribe is disabled with no file; during a run the UI is
  blocked and the meeting reaches finished only after transcription and
  diarization both complete.
- **F004-R9:** the status bar shows an indeterminate spinner with a per-second
  timer across the transcription and speaker-identification phases; it
  shows finished, creating-MFU, and the no-model warning in the right states.
- **F004-R10:** an edited segment persists and is present after reopening with no
  save action.
- **F004-R11:** the center pane hosts editable `m:ss` segments; with F002's
  diarization those segments group into colored per-speaker bubbles.
- **F004-R12:** with the source absent, the meeting opens, Transcribe is disabled,
  and a note explains why.
- **F004-R13:** exported `.md`/`.txt` contains the current transcript with speaker
  labels and timestamps, and the MFU notes when present.
- **F004-R14:** pressing Transcribe on a meeting that already has a transcript
  prompts before replacing it (and any MFU); cancelling the prompt keeps the
  existing content.

## Constraints

- SQLite via `rusqlite` (bundled), reusing VoicePilot patterns (ADR-008); left
  list mirrors VoicePilot's Sessions list.
- Reference the original source path; audio is not copied (ADR-008).
- Auto-save model — no explicit save/discard (ADR-008).
- Manual, UI-blocking Transcribe that runs to completion; MFU trigger is manual too (ADR-010,
  F003).
- Heavy work off the async reactor; a Tauri phase event marks the move into
  diarization, while completion or failure returns through the invoke result
  (`architecture.md`).

## Out of Scope

- Batch/queued processing of multiple files, or more than one file per meeting.
- In-app audio playback; copying audio into the library.
- Per-speaker colored bubbles are produced by F002 (also M2); MFU generation is
  F003/M3 — this feature hosts/stores their data but does not produce it.
