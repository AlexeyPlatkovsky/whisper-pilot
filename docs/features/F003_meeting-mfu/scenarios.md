# F003 Structured Meeting MFU — Scenarios

## Scenarios

### F003-S1: Structured MFU are generated locally on demand

Covers: F003-R1, F003-R2, F003-R6

```gherkin
Scenario: Five-section MFU generate on Create MFU, on-device
  Given a finalized transcript
    And the local LLM model is available
  When the user presses Create MFU
  Then the UI is blocked with a spinner and a live timer
    And when generation finishes, structured MFU appear in the MFU section
       with summary, key decisions, action items, open questions, and participants
    And no network request is made
```

### F003-S2: MFU are editable in place and persist

Covers: F003-R3

```gherkin
Scenario: An edited MFU section is auto-saved
  Given generated MFU
  When the user edits the action-items section
    And reopens the meeting
  Then the edited action items are present
```

### F003-S3: Regenerate guards manual edits

Covers: F003-R4

```gherkin
Scenario: Regenerate asks before replacing edited content
  Given MFU the user has edited
  When the user triggers regenerate
  Then a confirmation is shown before the edited content is replaced
```

### F003-S4: Copy MFU to clipboard

Covers: F003-R5

```gherkin
Scenario: Copy places the current MFU on the clipboard
  Given MFU (edited or not)
  When the user triggers copy
  Then the current MFU text is on the system clipboard
```

## Manual Verification Checklist

- [ ] (F003-R1) On a real Russian meeting transcript, the five sections capture
      the actual decisions and action items well enough to edit rather than
      rewrite; owners are attributed sensibly (best with F002 labels present).
- [ ] (F003-R2) Create MFU is disabled until transcription finishes; pressing it
      blocks the UI (spinner + timer) and renders MFU below the transcript.
- [ ] (F003-R6) A network monitor shows no traffic during generation.
- [ ] (F003-R4) Regenerating after manual edits prompts before overwriting.
