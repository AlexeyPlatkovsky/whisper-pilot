# F001 File Transcription — Tasks

M1 was implemented directly as a walking skeleton before TaskPilot tracking was
introduced for this project, so these tasks are recorded as **as-built** with no
TaskPilot IDs. They document what exists, for traceability.

| ID | Task | Implements | Depends on | TaskPilot |
| --- | --- | --- | --- | --- |
| F001-T1 | ffmpeg normalize + WAV decode to 16 kHz mono f32 (`audio.rs`) | F001-R2 | — | — (as-built) |
| F001-T2 | Whisper full-file transcription → timestamped segments (`transcribe.rs`) | F001-R3 | F001-T1 | — (as-built) |
| F001-T3 | IPC commands + app state + lazy model cache (`lib.rs`) | F001-R1, F001-R3, F001-R5 | F001-T2 | — (as-built) |
| F001-T4 | UI: add-file, transcribe, editable segments, save (`App.tsx`, `ipc.ts`) | F001-R1, F001-R4, F001-R5 | F001-T3 | — (as-built) |
| F001-T5 | Error type + actionable surfacing (`error.rs` + UI banner) | F001-R6 | — | — (as-built) |
| F001-T6 | End-to-end pipeline test (file → segments) | F001-R2, F001-R3 | F001-T2 | — (as-built) |

## MFU

- Verified end-to-end: a 1-minute file transcribes in ~3.5 s on Metal with
  clean, timestamped output.
- Follow-on (not part of M1): transcription progress reporting; own model
  management.
