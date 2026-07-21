# Design

Owns the product/UX design: flows, screens, and states. Technical structure is
in `architecture.md`; there is no separate design-system doc yet, so token and
style detail lives here until it grows enough to warrant `design-book.md`.

## UX Principles

- **One obvious action.** The empty state has a single primary action: add a
  file. Nothing else competes for attention until there is a transcript.
- **Progress is visible.** Transcription takes minutes; the UI always shows that
  work is happening and on which file.
- **Edit in place.** The transcript and summary are edited where they are read,
  not in a separate mode.
- **Russian-first surface.** UI copy is Russian by default, matching the primary
  transcription language.

## User Flows

### Transcribe a file (M1)

1. User clicks **Добавить файл** (Add file).
2. Native file picker opens, filtered to audio/video formats.
3. On selection, the app shows a transcribing state naming the file.
4. When done, the transcript appears as timestamped, editable segments.
5. User edits any segment text in place.
6. User clicks **Сохранить** (Save) and chooses a destination `.txt`.

### Review by speaker (M2)

1. After transcription, segments are grouped and labelled by speaker
   (Спикер 1, Спикер 2, …) as a chat.
2. User renames a speaker once; the label updates for all of that speaker's
   segments.
3. User edits and saves; the saved file carries the speaker labels.

### Summarize (M3)

1. Below the transcript, a **summary / MFU** section generates automatically (or
   on demand) from the finalized transcript.
2. User edits the summary in place.
3. User clicks copy to place the summary on the clipboard.

## Key Screens / Views

| Screen / View | Purpose | Entry point |
| --- | --- | --- |
| Empty state | Prompt to add the first file | App launch with no transcript |
| Transcribing state | Show that a named file is being processed | After a file is chosen |
| Transcript view | Read/edit timestamped (M2: per-speaker) segments; save | After transcription completes |
| Summary section (M3) | Read/edit/copy the generated summary | Below the transcript view |

## States

For the transcript view and its actions:

- **Empty** — centered prompt: "Добавьте аудио или видео файл, чтобы получить
  транскрипцию." Only the Add file action is enabled; Save is disabled.
- **Loading** — a banner: "Транскрибирую <файл>… это может занять несколько
  минут." Add file and Save are disabled while busy. (Roadmap: replace the
  indeterminate banner with real progress via Whisper's progress callback.)
- **Error** — a red banner carrying the `AppError` message (e.g. ffmpeg missing,
  model not found). The prior transcript, if any, is preserved; the user can
  retry with another file.
- **Populated** — the file name, then the list of segments; Save enabled.

## Interaction Patterns

- **Segment editing** — each segment is an auto-sizing text field prefixed by its
  timestamp (`m:ss`). Editing mutates in-memory state; Save serializes the
  current text.
- **Speaker rename (M2)** — inline rename on a speaker label; a single mapping
  applies the new name everywhere that speaker appears.
- **Copy (M3)** — one-click copy of the summary to the clipboard.
- **Feedback** — long operations always surface a banner; failures are shown, not
  swallowed.

## Accessibility

- Full keyboard operability: the primary actions are real buttons; editable
  segments are standard text fields in reading order.
- Sufficient contrast in both light and dark schemes (the UI follows the OS
  color scheme).
- Speaker distinction (M2) must not rely on color alone — labels carry the
  speaker name as text.
