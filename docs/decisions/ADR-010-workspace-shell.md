# ADR-010: Two-pane meeting workspace shell with manual Transcribe/MFU triggers

- **Status:** accepted
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

M1 was a single-screen, wizard-like flow: pick a file and it transcribes
immediately, with nothing kept. With the library (ADR-008) the app needs a
durable home for many meetings and a predictable place to run the two long,
GPU-heavy operations (transcription and MFU generation) without the UI acting on
its own. We also needed to settle the product's core noun and what the transcript
area shows now that diarization (F002) is part of M2.

## Decision

Adopt a **persistent two-pane shell**, mirroring VoicePilot's Sessions layout:

- **Left pane** — a **Meetings** list with `+ New meeting`, per-row rename
  (modal, ≤120 chars, non-empty) and delete (confirmation). The pane is
  collapsible via a header **toggle** placed immediately after the macOS traffic
  lights; the collapsed state persists across restarts.
- **Right pane** — the active meeting: a header (meeting label with
  edit/copy/delete, model switcher, language selector, Transcribe, Stop, Create
  MFU), a **status bar**, the transcript, and the **MFU section** beneath it.
- **"Meeting"** is the single product noun — UI copy, data model, and IPC all use
  it (supersedes "document"; see ADR-008 wording).
- **Manual, explicit, UI-blocking operations.** Transcription runs only on
  **Transcribe** and can be stopped; MFU is generated only on **Create MFU**
  (enabled after transcription finishes) and cannot be cancelled. While either
  runs, the UI is blocked except the control that tracks/stops it, and progress
  is always shown (a real progress bar, or a spinner with a live 1-second timer).
- **Transcript rendering.** The transcript renders as a per-speaker chat of
  **colored bubbles** (10 predefined shades) from **M2**, since diarization
  (F002) is now part of M2. F004 owns the editable segment surface; F002 owns the
  bubble grouping/coloring.

## Consequences

- Past work is always in reach; navigation is state, not screens.
- Decoupling attach → Transcribe lets the user set model/language first and makes
  runs deliberate; a manual MFU trigger (vs auto-on-completion) supersedes the
  earlier F003-R2 auto-generation and matches the button-driven header.
- No MFU cancel means a stuck LLM run is a force-quit — acceptable for the MVP,
  revisit if runs prove long or flaky.
- Diarization moved into M2 (from a later milestone), so M2 ships the colored
  per-speaker bubble transcript directly; sherpa-onnx integration lands earlier
  and M2 is correspondingly larger.

## Alternatives Considered

- **Keep the M1 wizard / auto-transcribe on file pick** — simplest, but gives no
  library home and removes user control over model/language; rejected with the
  library.
- **Auto-generate MFU on completion** (original F003-R2) — one less click, but
  fights the "explicit, blocking action" model and the disabled-until-finished
  button; rejected in favor of the manual Create MFU button.
- **Keep diarization in a later milestone and ship plain segments in M2** —
  smaller M2, but the transcript would show no speakers and the bubble UI would
  land later; superseded by the decision to move F002 into M2.
- **Keep "document" as the noun** — matches the SQLite table's generic sense, but
  "meeting" is the user's language and the product's subject; renamed everywhere.
