# ADR-011: Settings screen — in-app model management, theming, i18n, English-default UI

- **Status:** accepted
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

The app previously assumed models were placed on disk manually (model management
was deferred), the UI followed only the OS color scheme, and UI copy was framed
as Russian-first. For a **beta-ready** app users need to obtain the models they
require, choose a theme, and read the UI in a language they understand — without
leaving the app. Beta and public **release** want different depth here.

## Decision

Introduce a **Settings** screen (F005), reached from a fixed header **gear**, with
four sections, phased across milestones:

- **AI models** — a **fixed, app-defined catalog** of the model(s) each task needs
  (transcription, diarization, notes). Each model has **Download** (fetch from a
  known URL, stream progress, **SHA-verify**, mark ready only on success) and
  **Delete**. Beta: **one model per task**. Release: **3–4 per task** with an
  **Active** radio. A missing required model disables/degrades its task.
- **Appearance** — **Light / Dark / System** in beta; **3–4 extra named themes**
  (each light + dark) at release.
- **App language** — the **UI** language, **English by default** (only option in
  beta); **Russian, Turkish, Spanish, German, French** at release. Independent of
  the **transcription** language (Russian default, ADR-007).
- **Update app** — **release only**: user-initiated check + apply.

Settings persist in a **key–value store** in the app support directory, applied
immediately and across restarts. Theming and i18n **scaffolding** ship in beta so
release adds only assets.

## Consequences

- Supersedes the deferred "own model-management UI/catalog" note and the "manual
  model placement" limitation: beta can download/delete each task's model in-app.
- Introduces the **only** network egress in the product — model downloads (and the
  release app-update). It is user-initiated, to known URLs, SHA-verified; user
  audio/transcripts/notes still never leave the device (`architecture.md`
  Security & Privacy).
- The **UI language** default flips to **English**, superseding the earlier
  "Russian-first surface" wording; transcription stays Russian by default.
- Beta stays lean (one model/task, three themes, one language) while the structure
  supports the fuller release surface.

## Alternatives Considered

- **Keep manual model placement** — no download UI, but not viable for a beta a
  tester can set up unaided; rejected.
- **Open/third-party model catalog** — flexible, but a support and safety burden;
  rejected in favor of a fixed app-defined list.
- **OS-scheme only (no theme setting)** — simplest, but users asked for explicit
  Light/Dark and, later, richer themes; rejected.
- **Russian-first UI** — the prior stance; superseded by English-default UI with
  release localization, per the product owner.
- **Auto-update** — convenient, but we prefer **user-initiated** update for a
  local-first desktop app.
