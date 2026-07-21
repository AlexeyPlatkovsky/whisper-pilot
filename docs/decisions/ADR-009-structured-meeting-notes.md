# ADR-009: Structured meeting notes (full set), editable

- **Status:** accepted
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

The product generates a summary from the transcript ("MFU"). The exact meaning of
MFU and the summary's shape were undefined. For work meetings, a free-form blurb
is less useful than an actionable follow-up.

## Decision

Generate **structured meeting notes** (not a free-form blurb), in **Russian**, with
a fixed section set:

- **Summary** — brief gist.
- **Key decisions.**
- **Action items** — each with an owner and a task.
- **Open questions.**
- **Participants.**

Notes are generated **on demand via the Create MFU action** (enabled after
transcription finishes; UI-blocking — ADR-010), **editable in place**
(auto-saved), **clearable**, and **regenerable** on demand. Generation is local
(llama.cpp / Qwen2.5 — ADR-006).

## Consequences

- The notes are directly useful as a meeting follow-up, not just a summary.
- Requires a defined prompt/template producing the five sections reliably in
  Russian; prompt design becomes real work (part of M3).
- The "Participants" and per-owner "Action items" sections benefit from speaker
  labels (M2), so the M3 notes read better after M2 though they do not require it.

## Alternatives Considered

- **Free-form short summary** — simplest, but omits decisions/action items that
  make the output actionable.
- **Core set (summary + decisions + action items)** — leaner, but the user chose
  the full set including open questions and participants.
- **User-configurable sections** — more flexible, deferred; a fixed set ships
  first.
