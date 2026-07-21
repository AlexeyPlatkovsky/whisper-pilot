# Roadmap

Owns phases, milestones, sequencing, and non-goals over time. Per-feature task
breakdown lives in `features/F<NNN>/tasks.md`; work-item status lives in
TaskPilot (`WP-<n>`).

## Release Stance

Pre-1.0, milestone-driven. Each milestone is independently runnable: the app
works end-to-end at every milestone boundary, gaining capability rather than
being rewritten. No public distribution or notarization in the current horizon.

## Phases

### M1 — File transcription (done)

- **Goal:** Add an audio/video file and get an accurate, editable, timestamped
  Russian transcript that can be saved.
- **Features:** [`features/F001_file-transcription/`](features/F001_file-transcription/requirements.md)
- **Exit criteria:** file → ffmpeg → Whisper (Metal, full-file, Russian) →
  editable segments → save, verified end-to-end on a real file.

### M2 — Speaker roles (planned)

- **Goal:** Attribute each segment to a speaker and render the transcript as an
  editable, per-speaker chat.
- **Features:** [`features/F002_speaker-diarization/`](features/F002_speaker-diarization/requirements.md)
  (TaskPilot epic `WP-1`).
- **Exit criteria:** sherpa-onnx diarization merged onto segments; consistent
  per-speaker attribution within a file; editable, persisted speaker labels.

### M3 — Summary / MFU (planned)

- **Goal:** Generate a short, editable, copyable summary of the transcript with a
  local LLM.
- **Features:** [`features/F003_summary-mfu/`](features/F003_summary-mfu/requirements.md)
- **Exit criteria:** llama.cpp (Qwen2.5) summary section below the transcript,
  editable and copyable, fully local.

## Sequencing & Dependencies

- M2 depends on M1's `Segment` stream (it attaches speakers to existing
  segments).
- M3 depends on a finalized transcript; it is independent of M2 and could ship
  before it, but reads better with speaker labels present.
- English-language support is a cross-cutting follow-on after M1–M3 are stable
  in Russian; it mainly touches model/language selection, not the pipeline shape.

## Non-Goals (Over Time)

- Live capture — never; it is a different product (`idea.md` non-goals).
- Cloud transcription/summarization — never in this product's local-first stance.
- Real-name speaker identification (voice enrollment) — deferred beyond M2;
  labels stay generic and user-renamed.
- Own model-management UI/catalog — deferred; M1 reuses an existing model path
  via `MFUPILOT_MODEL_PATH`.
