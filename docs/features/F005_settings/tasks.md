# F005 Settings — Tasks

Beta scope (M2) is tracked under TaskPilot epic **WP-33**. Release scope (M3) is
unscheduled in TaskPilot until M3 is picked up (TaskPilot column `—`), mirroring
F003.

| ID | Task | Implements | Milestone | Depends on | TaskPilot |
| --- | --- | --- | --- | --- | --- |
| F005-T1 | Settings key–value store + get/set IPC (theme, ui_language, active models) | F005-R2 | M2 | — | WP-37 |
| F005-T2 | Settings screen shell + header gear entry + section navigation | F005-R1 | M2 | F005-T1 | WP-38 |
| F005-T3 | AI models: per-task list + Download (fetch + SHA verify + progress) | F005-R3 | M2 | F005-T2 | WP-39 |
| F005-T4 | AI models: Delete + missing-model availability wiring (into F004/F002) | F005-R4 | M2 | F005-T3 | WP-40 |
| F005-T5 | Appearance: light/dark/system theme apply + persist | F005-R6 | M2 | F005-T1 | WP-41 |
| F005-T6 | App language: i18n scaffolding, English default | F005-R8 | M2 | F005-T1 | WP-42 |
| F005-T7 | AI models: Active radio for multi-model tasks (3–4/task) | F005-R5 | M3 | F005-T4 | — (unscheduled) |
| F005-T8 | Appearance: 3–4 extra themes, each light + dark | F005-R7 | M3 | F005-T5 | — (unscheduled) |
| F005-T9 | App language: add Russian, Turkish, Spanish, German, French | F005-R9 | M3 | F005-T6 | — (unscheduled) |
| F005-T10 | Update app: check for and apply updates | F005-R10 | M3 | F005-T2 | — (unscheduled) |
| F005-T11 | Appearance: configure current Meeting and Streaming status colors | F005-R11 | M2 | F005-T1, F005-T2 | WP-88 |

## MFU

- TaskPilot **features** under epic WP-33: **WP-34** settings shell & persistence
  (WP-37, WP-38), **WP-35** AI model management (WP-39, WP-40), **WP-36**
  appearance & app language (WP-41, WP-42).
- Model download/delete (T3/T4) supersedes the earlier "manual model placement"
  limitation and the deferred model-management detail; the catalog is fixed and
  app-defined (ADR-011).
- Build the store (T1) first; the shell, model management, theme, and language all
  read/write through it.
- The beta introduces theming and i18n **scaffolding** so the release tasks
  (T7–T10) add assets, not structure.
