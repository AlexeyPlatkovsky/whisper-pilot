# F004 Library & Workspace — Tasks

TaskPilot epic **WP-11** owns work status; each task references its `WP-<n>` ID.

| ID | Task | Implements | Depends on | TaskPilot |
| --- | --- | --- | --- | --- |
| F004-T1 | SQLite store: meetings/segments/notes schema + CRUD (`store.rs`) | F004-R1 | — | WP-16 |
| F004-T2 | Auto-save wiring for segment/label/notes edits | F004-R10 | F004-T1 | WP-17 |
| F004-T3 | `transcribe_meeting(id)` command with phase transition and completion through its invoke result | F004-R1, F004-R8 | F004-T1 | WP-18 |
| F004-T4 | Remove unsafe in-process Meeting Stop/callback plumbing; isolated cancellation follows in WP-87 | F004-R8 | F004-T3 | WP-19, WP-86 |
| F004-T5 | Transcription language auto-detection (no user selection) | F004-R6 | — | WP-20 |
| F004-T6 | `list_meetings`/`open_meeting` + meetings list UI + **+ New meeting** | F004-R3 | F004-T1 | WP-21 |
| F004-T7 | Rename modal (≤120 chars, non-empty) + delete with confirmation (list + header) | F004-R3, F004-R4 | F004-T6 | WP-22 |
| F004-T8 | Source-file-missing state (disable re-transcribe) | F004-R12 | F004-T6 | WP-23 |
| F004-T9 | `export_meeting` to Markdown/plain text + header copy-transcript | F004-R13, F004-R4 | F004-T6 | WP-24 |
| F004-T10 | Two-pane shell + collapsible left pane (persisted) + panel toggle after traffic lights + logo | F004-R2 | — | WP-26 |
| F004-T11 | Meeting header controls + model switcher (default `large`; hidden + status-bar warning if none) | F004-R4, F004-R5 | F004-T10 | WP-27 |
| F004-T12 | Attach-file flow + status-bar file chip with × (one file per meeting) | F004-R7 | F004-T10 | WP-28 |
| F004-T13 | Status bar states (waiting / attached / transcribing / finished / creating-MFU / no-model) | F004-R9 | F004-T10 | WP-29 |
| F004-T14 | Transcript surface: editable (`m:ss`) segments in the center pane (F002 groups them into colored speaker bubbles) | F004-R11 | F004-T10 | WP-30 |
| F004-T15 | Re-transcribe guard: confirm before replacing an existing transcript/MFU | F004-R14 | F004-T3 | WP-32 |

## Notes

- TaskPilot **features** under epic WP-11: WP-12 persistence/meeting model,
  WP-13 transcription-to-meeting, WP-14 library UI, WP-15 export, and **WP-25
  workspace shell/header/status-bar UI** (tasks WP-26…WP-30).
- Build the store (T1) and the shell (T10) first; the list, header, attach flow,
  and transcript render hang off the shell, while the transcription commands hang
  off the store.
- This milestone reworks M1's stateless UI into the meeting-centric two-pane
  shell; M1's transcription/ffmpeg/whisper core (F001) is reused unchanged
  underneath.
- Per-speaker colored bubbles are F002 (also M2), rendered over the T14 segment
  surface; the MFU section's generation is F003/M3.
