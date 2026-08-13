# F005 Settings — Requirements

## Summary

A **Settings** screen with four sections: **AI models** (download/delete the model
each task needs; at release, pick an Active model among several per task),
**Appearance** (themes), **App language** (the UI language), and **Update app**
(release only). Settings persist locally and apply immediately. This feature
spans two milestones: a **Beta scope (M2)** and a **Release scope (M3)**, marked
per requirement below.

## Serves

- `idea.md` scope: "a Settings screen: AI models … Appearance … App language …
  Update app"; "one language setting — the app UI language … the transcription
  language is not a setting at all".
- `roadmap.md` phases: **M2 — Beta** (beta scope) and **M3 — Release** (release
  scope).
- ADR-011 (settings, model management, theming, i18n, update).
- TaskPilot epic **WP-33** (beta scope).

## Functional Requirements

| ID | Requirement | Milestone | Priority |
| --- | --- | --- | --- |
| F005-R1 | The system shall provide a **Settings** screen, reachable from a fixed entry point (a gear control in the header), with sections **AI models**, **Appearance**, and **App language** (and **Update app** at release). | M2 | must |
| F005-R2 | The system shall persist all settings locally and apply them immediately and across restarts. | M2 | must |
| F005-R3 | The **AI models** section shall list, per task type (transcription, diarization; MFU at M3), the model(s) that task needs, each with **Download** and **Delete** actions; Download fetches the model and **verifies it (SHA)** with visible progress, Delete removes the local file. In Beta there is exactly **one model per task**. | M2 | must |
| F005-R4 | When a task's required model is **not downloaded**, the system shall make that obvious in the section and reflect it in the task's availability (its action is disabled or degrades per that feature's rules). | M2 | must |
| F005-R5 | At release, a task may list **several models (3–4)**; each downloaded model shall carry an **Active** radio selecting which one the task uses. | M3 | should |
| F005-R6 | The **Appearance** section shall offer **Light**, **Dark**, and **System** themes; the choice applies immediately and persists (System follows the OS scheme). | M2 | must |
| F005-R7 | At release, Appearance shall add **3–4 extra named themes**, each available in a **light and a dark** variant. | M3 | could |
| F005-R8 | The **App language** section shall set the **UI language**, defaulting to **English** (the only option in Beta); this is independent of the transcription language. | M2 | must |
| F005-R9 | At release, App language shall add **Russian, Turkish, Spanish, German, French**. | M3 | should |
| F005-R10 | At release, an **Update app** section shall let the user check for and apply application updates. | M3 | should |
| F005-R11 | The **Appearance** section shall list each current semantic Meeting and Streaming status once and allow the user to save an opaque `#RRGGBB` color for it. The two-column list sorts statuses alphabetically and fills each row from left to right. Each saved color applies to every current matching status surface and persists locally. The picker opens in an anchored popover; Cancel changes nothing; the contrast ratio is rounded half-up to two decimal places before comparison to `4.50:1`. A value below that practical threshold shows an inline warning in the form `Low contrast (2.49<4.50:1) against the current theme background.`; the color may still be saved. **Reset all colors** asks for confirmation and restores the documented built-in mapping; a non-default row shows a revert icon after its hex code that restores the default without confirmation. | M2 | should |

## Acceptance Criteria

- **F005-R1:** the gear opens Settings; the three (Beta) sections are present; the
  entry point's position is fixed.
- **F005-R2:** a changed theme/language/model selection survives an app restart.
- **F005-R3:** each task shows its required model with Download/Delete; Download
  opens a blocking progress dialog and only marks the model ready after SHA
  verification (dismissing the dialog early does not cancel the download); Delete
  asks for confirmation, then removes the file and returns the model to the
  not-downloaded state.
- **F005-R4:** with a task's model absent, the section flags it and the task's
  action is disabled/degraded (e.g. Transcribe disabled without the Whisper
  model; diarization degrades per F002-R7).
- **F005-R5:** (release) a task with several downloaded models shows an Active
  radio; selecting one makes the task use it.
- **F005-R6:** switching Light/Dark/System changes the app theme at once and
  persists; System tracks OS light/dark changes.
- **F005-R8:** the UI renders in English by default; the language detected for a
  transcript never changes the UI language.
- **F005-R9 / R7 / R10:** (release) the added languages/themes are selectable and
  apply; Update can check for and apply an update.
- **F005-R11:** every current status has one color control; the alphabetized list
  fills rows left-to-right across two columns; valid saved colors apply consistently
  to matching Meeting/Streaming widgets and indicators and persist; Cancel and
  rejected input leave the prior mapping intact; a low-contrast warning uses the
  half-up rounded two-decimal ratio and compares it to 4.50:1 without requiring
  confirmation; Reset all restores the built-in mapping.

## Constraints

- The model list per task is a **fixed, app-defined catalog** (ADR-011); users do
  not add arbitrary third-party models. Downloads come from known URLs with SHA
  verification, reusing the Whisper/sherpa asset-handling pattern.
- Settings persist in the app support directory (a small key–value store),
  alongside the SQLite library (`architecture.md`).
- Theming and i18n scaffolding are introduced in Beta even with a single language,
  so release only adds assets, not structure.
- The UI language is the **only** language setting. The transcription language is
  not configurable: Whisper detects it per run and the meeting records the
  result (ADR-012).

## Out of Scope

- Adding arbitrary/third-party models or editing the catalog.
- Per-meeting theme or language overrides (settings are app-wide).
- Cloud sync of settings or models.
- Custom status labels, icons, ordering, creation, or colors for statuses added after
  the delivered registry.
- Auto-update without user action (Update is user-initiated).
