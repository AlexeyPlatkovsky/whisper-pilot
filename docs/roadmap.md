# Roadmap

Owns phases, milestones, sequencing, and non-goals over time. Per-feature task
breakdown and work-item status both live in TaskPilot (`WP-<n>`).

## Release Stance

Pre-1.0, milestone-driven. Each milestone is independently runnable: the app
works end-to-end at every milestone boundary, gaining capability rather than
being rewritten. No public distribution or notarization in the current horizon.

## Phases

### M1 — Transcription core (done)

- **Goal:** Add an audio/video file and get an accurate, editable, timestamped
  Russian transcript that can be exported. Stateless (no library yet).
- **Feature:** file transcription core.
- **Exit criteria:** file → ffmpeg → Whisper (Metal, full-file, Russian) →
  editable segments → save, verified end-to-end on a real file. *(As shipped in
  M1; the forced Russian decode was superseded by ADR-012 — the language is now
  auto-detected.)*

### M2 — Library, workspace & speaker roles — **Beta** (next)

- **Goal:** Turn the stateless flow into a persisted two-pane workspace **and**
  attribute the transcript by speaker, to a **beta-ready** app. A local library of
  **meetings** with reopen/rename/delete, **auto-saved** edits, a manual
  **Transcribe** action with an indeterminate running status, automatic language detection
  (safe cancellation is deferred to WP-87's isolated worker),
  and **Markdown / plain-text export**; local **diarization** so the
  transcript renders as a per-speaker chat of **colored bubbles**; plus a
  **Settings** screen (beta scope): **AI models** download/delete (one model per
  task), **Appearance** (light / dark / system themes), and **App language**
  (English UI).
- **Features:** library & workspace (TaskPilot epic `WP-11`), speaker
  diarization (TaskPilot epic `WP-1`), and the beta scope of Settings
  (TaskPilot epic `WP-33`).
- **Exit criteria:** transcriptions persist as meetings; edits auto-save;
  meetings reopen (with a source-missing state); export produces `.md`/`.txt`;
  segments render as colored per-speaker bubbles with editable, persisted labels;
  Settings can download/delete each task's model, switch light/dark/system theme,
  and the UI is English.

### M3 — MFU, full settings & polish — **Release**

- **Goal:** Complete the app for a **public release**: structured meeting MFU,
  the full Settings surface, a localized UI, richer themes, and in-app update.
- **Features:** structured meeting MFU (structured, editable, copyable MFU
  via a local LLM) and the release scope of Settings: **AI models** with an
  **Active** choice among 3–4 models per task; **Appearance** with 3–4 extra
  themes (each in light and dark); **App language** adding Russian, Turkish,
  Spanish, German, French; and **Update app**.
- **Exit criteria:** MFU generate in Russian below the transcript, editable and
  copyable; each task can hold several models with an Active selection; extra
  themes and UI languages are selectable; the app can check for and apply updates.

## Sequencing & Dependencies

- **Within M2:** build the library/meeting model first (the durable place
  speakers and MFU are stored), then layer diarization onto M1's `Segment`
  stream so bubbles render against persisted segments; Settings (beta) supplies
  the models those pipelines need (download/delete) and the theme/UI-language
  choices.
- M3 reads a finalized (M2-persisted) transcript; independent of the MFU model
  but reads better with the M2 speaker labels present. The release Settings scope
  builds directly on the beta Settings shell.
- **UI language vs transcription language:** the **app UI** defaults to **English**
  (localized further at release) and is a setting; the **transcription** language
  is always auto-detected and is not selectable (ADR-012, superseding ADR-007 on
  this point).

## Non-Goals (Over Time)

- Meeting becoming a real-time transcriber — never; its accuracy comes from
  full-file batch processing (ADR-002), unchanged. Streaming (ADR-014) is a
  separate, additive capability with its own quality-over-latency priority;
  see `idea.md` for its scope. Streaming's own phase/milestone placement is
  not yet decided — its epic (WP-68) is tracked in TaskPilot pending that.
- Cloud transcription/summarization/storage — never; local-first stance
  (applies to Meeting and Streaming alike).
- Real-name speaker identification (voice enrollment) — deferred beyond M2.
- Speaker reassignment / merge — deferred beyond M2; misattributed segments stay
  as attributed.
- In-app audio playback — deferred.
- Batch/queued multi-file processing — deferred; one file at a time.
- Model **catalog authoring** (adding arbitrary third-party models) — out of
  scope; Settings manages a **fixed, app-defined** model list per task (F005).
