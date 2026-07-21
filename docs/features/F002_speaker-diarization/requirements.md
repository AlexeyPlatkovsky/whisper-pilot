# F002 Speaker Diarization — Requirements

## Summary

Attribute each transcript segment to a speaker ("by roles") using local
sherpa-onnx diarization, and render the transcript as an editable, per-speaker
chat with renamable labels. Builds on F001's segment stream.

## Serves

- `idea.md` scope: "a speaker-attributed, editable transcript".
- `roadmap.md` phase: **M2 — Speaker roles**.
- TaskPilot epic **WP-1**.

## Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| F002-R1 | The system shall produce ordered speaker turns `{ start_ms, end_ms, speaker }` for a file using local sherpa-onnx models. | must |
| F002-R2 | The system shall attribute each transcription segment to the speaker whose turns maximally overlap it, with deterministic tie-breaking. | must |
| F002-R3 | The system shall render the transcript as a per-speaker chat, grouped and labelled by speaker, preserving in-place text editing. | must |
| F002-R4 | The system shall let the user rename a speaker once so the label applies to all that speaker's segments and persists on save. | must |
| F002-R5 | The system shall support a provided speaker count or auto-detect it when none is given. | should |

## Acceptance Criteria

- **F002-R1:** a multi-speaker file yields ordered turns covering the audio;
  models are downloaded and SHA-verified before use.
- **F002-R2:** unit tests prove overlap assignment and tie-breaking for full
  overlap, split overlap, gap, and tie inputs; every emitted segment has a
  stable speaker id.
- **F002-R3:** segments render grouped by speaker with a visible speaker label
  and retained timestamps; editing a segment still works.
- **F002-R4:** renaming Спикер N updates every attributed segment and the change
  is present in the saved output.
- **F002-R5:** given N, exactly N speakers are used; without N, a plausible count
  is detected.

## Constraints

- Diarization is local sherpa-onnx (segmentation + embedding), no Python
  (ADR-005); models managed like the Whisper artifact.
- The merge is a pure function with unit tests (`testing.md`).
- Speaker labels are generic and user-renamed; no real-name identification
  (`idea.md` out-of-scope).

## Out of Scope

- Voice enrollment / real-name identification.
- Cross-file speaker consistency (labels are per-file).
