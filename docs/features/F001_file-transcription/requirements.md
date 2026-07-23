# F001 File Transcription — Requirements

## Summary

Add a local audio or video file and receive an accurate, timestamped transcript
in the language Whisper detects in it — Russian being what the app is tuned and
validated for — editable in place and saveable to a text file. This is the M1
foundation on which speaker roles (F002) and summarization (F003) build.

## Serves

- `idea.md` scope: "Transcribing local audio and video files"; "offline,
  on-device transcription"; "editable transcript that can be saved".
- `roadmap.md` phase: **M1 — File transcription**.

## Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| F001-R1 | The system shall let the user choose a local audio or video file through a native file picker. | must |
| F001-R2 | The system shall normalize any input (audio or video) to 16 kHz mono PCM via ffmpeg before transcription. | must |
| F001-R3 | The system shall transcribe the entire file into timestamped segments using Whisper on Metal with beam search, detecting the spoken language from the audio rather than being told it (ADR-012). | must |
| F001-R4 | The system shall display segments as editable text, each with its start timestamp, and let the user edit any segment. | must |
| F001-R5 | The system shall save the current (possibly edited) transcript to a user-chosen text file. | must |
| F001-R6 | The system shall surface ingestion and model errors (ffmpeg missing, model missing) as actionable messages without crashing. | must |

## Acceptance Criteria

- **F001-R1:** choosing a file returns its path; cancelling changes nothing.
- **F001-R2:** a video input yields the same 16 kHz mono samples an audio input
  would; the temporary WAV is deleted afterward.
- **F001-R3:** a real file produces ≥1 non-empty segment with ordered,
  non-degenerate timestamps; Russian audio reads accurately.
- **F001-R4:** editing a segment updates in-memory state and is reflected on save.
- **F001-R5:** the saved file contains the current segment text joined in order.
- **F001-R6:** with ffmpeg absent or the model missing, a specific error banner
  appears and prior transcript state is preserved.

## Constraints

- Full-file batch decoding only — no VAD, streaming, or real-time path
  (ADR-002).
- Whisper `large-v3-turbo` via `whisper-rs` `metal`; model path overridable via
  `WHISPERPILOT_MODEL_PATH` (ADR-003).
- ffmpeg is a required external dependency on PATH (ADR-004).
- Heavy work runs off the async reactor (`spawn_blocking`) to keep IPC/UI
  responsive.

## Out of Scope

- Speaker attribution (F002) and summarization (F003).
- Progress reporting during transcription (roadmap: add via Whisper progress
  callback).
- Own model catalog/download UI (reuses an existing artifact for M1).
