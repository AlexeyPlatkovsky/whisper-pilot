# ADR-008: Persisted meeting library (SQLite), reference-only audio, auto-save

- **Status:** accepted
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

M1 was stateless: transcribe a file, edit, export, done — nothing persisted. For
repeated meetings, users need to reopen, re-edit, and keep past transcriptions,
and to attach speakers (M2) and MFU (M3) durably to a meeting.

## Decision

Introduce a persisted **library** of **meetings** (one meeting = one
transcription) in a local **SQLite** database (via `rusqlite`, reusing
VoicePilot's patterns). Meetings **reference the original source file path**
rather than copying audio. Edits
(transcript, speaker labels, MFU) **auto-save** to the DB; export is a separate
explicit action. Processing remains **one file at a time**.

## Consequences

- The app becomes a two-pane meeting workspace: meetings list, reopen, rename,
  delete.
- Small storage footprint (no audio copies); but a meeting whose source moved or
  was deleted cannot be re-transcribed — a defined "source missing" state.
- Auto-save gives a modern, no-lost-work feel and removes save-state UI; it means
  every edit writes to the DB.
- M2 diarization and M3 MFU attach their data (speaker ids, MFU) to the
  persisted meeting.

## Alternatives Considered

- **Export-only / stateless** (M1's model) — simplest, but users cannot reopen or
  accumulate work; rejected once a library was chosen.
- **Copy audio into the library** — robust to source moves and enables re-run and
  playback, but costs significant disk for large media; rejected in favor of
  reference-only for now (revisit if re-transcribe/playback demand grows).
- **Explicit save/discard** — more control, but risks lost work and adds
  save-state UI; auto-save preferred.
- **Batch/queue of files** — deferred; one-at-a-time keeps the workspace simple.
