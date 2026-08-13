# F002 Speaker Diarization — Requirements

## Summary

Attribute each transcript segment to a speaker ("by roles") using local
sherpa-onnx diarization, and render the transcript as a per-speaker chat with
renamable labels. Builds on F001's segments, now persisted per meeting by F004;
speaker ids and label renames are stored on the meeting and auto-saved.

## Serves

- `idea.md` scope: "a speaker-attributed, editable transcript".
- `roadmap.md` phase: **M2 — Library, workspace & speaker roles** (alongside
  F004).
- `design.md`: the per-speaker colored-bubble transcript.
- TaskPilot epic **WP-1**.

## Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| F002-R1 | The system shall produce ordered speaker turns `{ start_ms, end_ms, speaker }` for a file using local sherpa-onnx models. | must |
| F002-R2 | The system shall attribute each transcription segment to the speaker whose turns maximally overlap it, with deterministic tie-breaking. | must |
| F002-R3 | The system shall render the transcript as a per-speaker chat of **colored bubbles** (10 predefined shades cycled across speakers), grouped and labelled by speaker, preserving in-place text editing. | must |
| F002-R4 | The system shall let the user rename a speaker once so the label applies to all that speaker's segments and persists on save. | must |
| F002-R5 | The system shall support a provided speaker count or auto-detect it when none is given. | should |
| F002-R6 | The system shall run diarization **automatically as part of the Transcribe flow** (after transcription, before the meeting is marked finished) and expose the phase transition within the same run. | must |
| F002-R7 | If diarization is unavailable or fails (missing models, engine error), the system shall still present the transcript as plain segments (no bubbles) with a detail, rather than failing the run. | must |

## Acceptance Criteria

- **F002-R1:** a multi-speaker file yields ordered turns covering the audio;
  models are downloaded and SHA-verified before use.
- **F002-R2:** unit tests prove overlap assignment and tie-breaking for full
  overlap, split overlap, gap, and tie inputs; every emitted segment has a
  stable speaker id.
- **F002-R3:** segments render grouped by speaker in colored bubbles (10 shades)
  with a visible speaker label and retained timestamps; editing a segment still
  works.
- **F002-R4:** renaming Спикер N updates every attributed segment and the change
  is present in the saved output.
- **F002-R5:** given N, exactly N speakers are used; without N, a plausible count
  is detected.
- **F002-R6:** bubbles appear at the end of a normal Transcribe run with no extra
  user action; the status changes to identifying speakers for the diarization
  phase and the Meeting run completes as one operation.
- **F002-R7:** with diarization models absent, the transcript still renders (plain
  segments) and the meeting is finished, with a detail explaining speakers are
  unavailable.

## Constraints

- Diarization is local sherpa-onnx (segmentation + embedding), no Python
  (ADR-005); models managed like the Whisper artifact.
- The merge is a pure function with unit tests (`testing.md`).
- Speaker labels are generic and user-renamed; no real-name identification
  (`idea.md` out-of-scope).

## Out of Scope

- Voice enrollment / real-name identification.
- Cross-file speaker consistency (labels are per-file).
