# Roadmap

Owns phases, milestones, sequencing, and non-goals over time. Per-feature task
breakdown lives in `features/F<NNN>/tasks.md`; work-item status lives in
TaskPilot (`WP-<n>`).

## Release Stance

Pre-1.0, milestone-driven. Each milestone is independently runnable: the app
works end-to-end at every milestone boundary, gaining capability rather than
being rewritten. No public distribution or notarization in the current horizon.

## Phases

### M1 — Transcription core (done)

- **Goal:** Add an audio/video file and get an accurate, editable, timestamped
  Russian transcript that can be exported. Stateless (no library yet).
- **Feature:** [`F001_file-transcription`](features/F001_file-transcription/requirements.md)
- **Exit criteria:** file → ffmpeg → Whisper (Metal, full-file, Russian) →
  editable segments → save, verified end-to-end on a real file.

### M2 — Library, workspace & speaker roles (next)

- **Goal:** Turn the stateless flow into a persisted two-pane workspace **and**
  attribute the transcript by speaker. A local library of **meetings** with
  reopen/rename/delete, **auto-saved** edits, a manual **Transcribe** action with
  **progress + Stop**, language selection (Russian / auto-detect), and **Markdown
  / plain-text export**; plus local **diarization** so the transcript renders as a
  per-speaker chat of **colored bubbles** with renamable labels.
- **Features:**
  [`F004_library-workspace`](features/F004_library-workspace/requirements.md)
  (TaskPilot epic `WP-11`) and
  [`F002_speaker-diarization`](features/F002_speaker-diarization/requirements.md)
  (TaskPilot epic `WP-1`, `WP-5…WP-10`).
- **Exit criteria:** transcriptions persist as meetings; edits auto-save;
  meetings reopen (with a source-missing state); export produces `.md`/`.txt`;
  segments are consistently attributed to distinct speakers within a file and
  render as colored per-speaker bubbles with editable, persisted labels.

### M3 — Structured meeting notes

- **Goal:** Generate structured, editable, copyable meeting notes (summary,
  decisions, action items, open questions, participants) with a local LLM.
- **Feature:** [`F003_meeting-notes`](features/F003_meeting-notes/requirements.md)
- **Exit criteria:** llama.cpp (Qwen2.5) notes generated in Russian below the
  transcript, editable and regenerable, copyable, fully local.

## Sequencing & Dependencies

- **Within M2:** build the library/meeting model first (the durable place
  speakers and notes are stored), then layer diarization onto M1's `Segment`
  stream so bubbles render against persisted segments.
- M3 reads a finalized (M2-persisted) transcript; independent of the notes model
  but reads better with the M2 speaker labels present.
- English-language support is a cross-cutting follow-on after M1–M3 are stable in
  Russian; it mainly touches language/model selection, not the pipeline shape.

## Non-Goals (Over Time)

- Live capture — never; a different product (`idea.md` non-goals).
- Cloud transcription/summarization/storage — never; local-first stance.
- Real-name speaker identification (voice enrollment) — deferred beyond M2.
- Speaker reassignment / merge — deferred beyond M2; misattributed segments stay
  as attributed.
- In-app audio playback — deferred.
- Batch/queued multi-file processing — deferred; one file at a time.
- Own model-management UI/catalog — deferred; reuses an existing model path.
