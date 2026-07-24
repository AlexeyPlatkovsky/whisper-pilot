# F004 Library & Workspace — Scenarios

## Scenarios

### F004-S1: Transcription is persisted as a library meeting

Covers: F004-R1

```gherkin
Scenario: A completed transcription appears in the library and survives restart
  Given a transcribed file
  When transcription completes
  Then a meeting titled after the source file name, with its source name, date,
       duration, and language appears in the Meetings list
    And it is still present after the app restarts
```

### F004-S2: Two-pane shell and left-panel toggle

Covers: F004-R2

```gherkin
Scenario: The left panel toggles and remembers its state
  Given the app is open with the Meetings list visible
  When the user clicks the panel toggle in the header
  Then the left panel hides and the toggle position does not move
  When the user restarts the app
  Then the panel remains hidden until toggled back
```

### F004-S3: Create, rename, and delete a meeting

Covers: F004-R3

```gherkin
Scenario: New meeting, rename validation, and confirmed delete
  Given the Meetings list
  When the user clicks "+ New meeting"
  Then an empty meeting is created and selected
  When the user opens rename and enters an empty or >120-character title
  Then Save is rejected
  When the user enters a valid title and saves
  Then the list shows the new title
  When the user deletes the meeting and confirms
  Then it is removed from the list and the database
```

### F004-S4: Header actions on a meeting

Covers: F004-R4

```gherkin
Scenario: Copy places the transcript on the clipboard
  Given an open meeting with a transcript
  When the user clicks copy on the meeting label
  Then the full transcript text is on the system clipboard
    And edit opens the rename modal, and delete asks for confirmation
```

### F004-S5: Model switcher

Covers: F004-R5

```gherkin
Scenario: Default model and the no-model case
  Given at least one Whisper model is installed
  Then the switcher defaults to the large model
  Given no Whisper model is installed
  Then the switcher is hidden and the status bar shows a warning
```

### F004-S6: Language is auto-detected, never chosen

Covers: F004-R6

```gherkin
Scenario: No language control exists
  Given a file to transcribe
  When the user looks for a language control
  Then there is none anywhere in the app

Scenario: The language is detected from the audio
  Given an English recording
  When it is transcribed
  Then Whisper detects English
  And the transcript is the English speech
  And the meeting records the detected code

Scenario: A meeting has no language until it is transcribed
  Given a meeting that has been created but never transcribed
  Then its stored language is the not-yet-detected value
  And detaching its source file returns it to that value
```

### F004-S7: Attach and detach a file

Covers: F004-R7

```gherkin
Scenario: One file at a time, shown in the status bar
  Given an empty meeting
  When the user attaches a file
  Then the status bar shows the file with an × and no transcription starts
  When the user attaches a second file
  Then it replaces the first
  When the user clicks ×
  Then the file is detached
```

### F004-S8: Transcribe blocks the UI and can be stopped

Covers: F004-R8

```gherkin
Scenario: Stopping a transcription leaves the library unchanged
  Given a meeting with a file attached
  When the user presses Transcribe
  Then the UI is blocked except the Stop control
  When the user presses Stop
  Then no meeting transcript is persisted and the library is unchanged
```

### F004-S9: Status bar reflects state

Covers: F004-R9

```gherkin
Scenario: The status bar shows progress or a timer
  Given a transcription is running
  Then the status bar shows a progress bar, or a spinner with a per-second timer
  When transcription finishes
  Then the status bar shows the finished state and Create MFU becomes enabled
```

### F004-S10: Edits auto-save

Covers: F004-R10

```gherkin
Scenario: An edit persists without a save action
  Given an open meeting
  When the user edits a segment's text
    And reopens the meeting later
  Then the edited text is present, with no save action having been taken
```

### F004-S11: Editable transcript surface with speaker bubbles

Covers: F004-R11

```gherkin
Scenario: The center pane hosts editable segments, grouped into speaker bubbles
  Given an opened meeting with a diarized transcript (F002)
  Then segments render as editable m:ss fields
    And they group into per-speaker colored bubbles
```

### F004-S12: Source file missing

Covers: F004-R12

```gherkin
Scenario: A meeting whose source is gone opens without re-transcribe
  Given a meeting whose source file has been moved or deleted
  When the user opens it
  Then the transcript is shown and editable
    And Transcribe is disabled with an explanatory note
```

### F004-S13: Export

Covers: F004-R13

```gherkin
Scenario: Export a meeting to Markdown and plain text
  Given an open meeting
  When the user exports it as Markdown, then as plain text
  Then each file contains the current transcript with speaker labels and
       timestamps, plus the MFU notes when present
```

### F004-S14: Re-transcribe guards existing content

Covers: F004-R14

```gherkin
Scenario: Re-running Transcribe warns before replacing
  Given a meeting that already has a transcript (and possibly an MFU)
  When the user presses Transcribe again
  Then a confirmation warns the existing transcript and MFU will be replaced
  When the user cancels
  Then the existing transcript and MFU are unchanged
```

## Manual Verification Checklist

- [ ] (F004-R1) A restart shows previously transcribed meetings in the list.
- [ ] (F004-R2) The panel toggle sits right after the traffic lights, never
      moves, and its collapsed state survives restart.
- [ ] (F004-R3) Rename rejects empty and >120-char titles; delete confirms.
- [ ] (F004-R5) With no model installed, the switcher is hidden and the status bar
      warns.
- [ ] (F004-R8) During a run the UI is blocked except Stop; cancel leaves no
      partial meeting.
- [ ] (F004-R9) The progress bar advances roughly with the file, or a spinner
      timer ticks each second.
- [ ] (F004-R12) Renaming/moving the source on disk yields the source-missing note
      on next open.
- [ ] (F004-R13) Exported Markdown renders correctly in a Markdown viewer.
