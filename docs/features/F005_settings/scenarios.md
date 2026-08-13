# F005 Settings — Scenarios

## Scenarios

### F005-S1: Open Settings and see the sections

Covers: F005-R1

```gherkin
Scenario: The gear opens Settings with the beta sections
  Given the app is open
  When the user clicks the gear entry point in the header
  Then the Settings screen opens with AI models, Appearance, and App language
```

### F005-S2: Settings persist across restart

Covers: F005-R2

```gherkin
Scenario: A changed setting survives a restart
  Given the user changes the theme and a model selection
  When the app restarts
  Then the previously chosen theme and model selection are still in effect
```

### F005-S3: Download and delete a task's model

Covers: F005-R3

```gherkin
Scenario: Download verifies, Delete removes
  Given the AI models section lists the transcription model as not downloaded
  When the user clicks Download
  Then a blocking dialog shows progress, and the model is marked ready only
    after SHA verification
  When the user dismisses the dialog with ✕ before it finishes
  Then the dialog closes but the download keeps running and the model still
    becomes ready in the background
  When the user clicks Delete
  Then a confirmation dialog asks before removing anything
  When the user confirms
  Then the local model file is removed and it returns to not-downloaded
```

### F005-S4: Missing model disables the task

Covers: F005-R4

```gherkin
Scenario: A task with no model is unavailable
  Given the transcription model is not downloaded
  Then the AI models section flags it as missing
    And Transcribe is disabled with the no-model warning (F004-R5)
  Given the diarization model is not downloaded
  Then transcription still runs and degrades to plain segments (F002-R7)
```

### F005-S5: Active model among several (release)

Covers: F005-R5

```gherkin
Scenario: Choosing the Active model for a task
  Given a task lists several downloaded models
  When the user selects one with the Active radio
  Then that task uses the selected model for its next run
```

### F005-S6: Light, Dark, and System themes

Covers: F005-R6

```gherkin
Scenario: Theme applies immediately and follows the OS for System
  Given the Appearance section
  When the user selects Dark
  Then the app switches to dark at once and stays dark after restart
  When the user selects System and the OS is in light mode
  Then the app follows the OS scheme
```

### F005-S7: Extra themes with light/dark variants (release)

Covers: F005-R7

```gherkin
Scenario: A named release theme in both variants
  Given the release themes are available
  When the user picks a named theme
  Then it renders correctly in both its light and dark variants
```

### F005-S8: UI language defaults to English and is independent

Covers: F005-R8

```gherkin
Scenario: UI English by default, independent of transcription language
  Given a fresh install
  Then the UI renders in English
  When a Russian recording is transcribed and detected as Russian
  Then the UI language stays English
```

### F005-S9: Additional UI languages (release)

Covers: F005-R9

```gherkin
Scenario: Selecting a release UI language
  Given the release languages are available
  When the user selects Russian for the app language
  Then the UI renders in Russian
```

### F005-S10: Update the app (release)

Covers: F005-R10

```gherkin
Scenario: Check for and apply an update
  Given the Update app section
  When the user checks for updates and one is available
  Then the user can apply it
```

### F005-S11: Configure a status color

Covers: F005-R11

```gherkin
Scenario: Save, revert, or cancel a status color
  Given Settings lists the current semantic statuses in alphabetical order,
    filling each two-column row from left to right, with their built-in colors
  When the user opens a status color popover, selects a valid #RRGGBB value,
    and saves it with sufficient contrast
  Then every matching Meeting and Streaming status surface uses the new color
    and it persists after restart
  When the selected color has insufficient contrast
  Then the popover shows an inline warning label with its calculated contrast ratio
    rounded half-up to two decimal places and compared with 4.50:1
    and the color may still be saved
  When the user clicks Reset all colors and confirms the modal
  Then every status returns to its documented built-in color
```

## Manual Verification Checklist

- [ ] (F005-R1) The gear sits in a fixed header position and opens Settings.
- [ ] (F005-R3) Interrupting a download leaves no half-written model marked ready;
      re-download works; SHA mismatch is reported, not silently accepted.
- [ ] (F005-R3) Dismissing the download dialog with ✕ does not cancel the
      download; Delete never removes a model without an explicit confirm.
- [ ] (F005-R4) With each model removed in turn, the dependent task behaves per its
      rule (Transcribe disabled; diarization degrades).
- [ ] (F005-R6) System theme tracks a live OS light↔dark switch.
- [ ] (F005-R8) No stray non-English UI strings with the default settings.
- [ ] (F005-R11) Canceling a color popover changes neither visible statuses nor
      stored values; malformed/alpha values are rejected; a low-contrast pick shows
      an inline warning label whose ratio is rounded half-up to two decimal places
      and compared with 4.50:1 but can still be saved; the alphabetized status list
      fills rows left-to-right across two columns; Reset all asks for
      confirmation; an individual persistence failure or Reset-all persistence failure
      restores the full prior mapping; missing/malformed legacy mappings silently use
      built-in defaults; and a later save wins over stale pending responses.
