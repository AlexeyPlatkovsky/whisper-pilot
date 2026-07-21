# F002 Speaker Diarization — Tasks

TaskPilot epic **WP-1** owns work status; each task references its `WP-<n>` ID.

| ID | Task | Implements | Depends on | TaskPilot |
| --- | --- | --- | --- | --- |
| F002-T1 | Add sherpa-onnx dependency + diarization model assets (download/verify) | F002-R1 | — | WP-5 |
| F002-T2 | `diarize_file`: run sherpa-onnx → ordered speaker turns; provided/auto count | F002-R1, F002-R5 | F002-T1 | WP-6 |
| F002-T3 | Turn↔segment merge algorithm (max-overlap, deterministic tie-break) + unit tests | F002-R2 | F002-T2 | WP-7 |
| F002-T4 | Thread speaker id through `Segment`/`TranscriptResult` and IPC | F002-R2 | F002-T3 | WP-8 |
| F002-T5 | Per-speaker chat rendering | F002-R3 | F002-T4 | WP-9 |
| F002-T6 | Editable, persisted speaker labels | F002-R4 | F002-T5 | WP-10 |

## Notes

- WP-2/WP-3/WP-4 are the TaskPilot **features** grouping these tasks under epic
  WP-1 (engine / merge / UI).
- T3 (merge) is the correctness-critical piece and must be pure + unit-tested
  before UI work.
- M1 (no diarization) behavior must remain valid when a segment has no speaker id.
