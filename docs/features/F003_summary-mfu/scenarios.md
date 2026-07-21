# F003 Summary / MFU — Scenarios

## Scenarios

### F003-S1: Transcript is summarized locally

Covers: F003-R1, F003-R5

```gherkin
Scenario: A summary is generated from the transcript on-device
  Given a finalized transcript
    And the local LLM model is available
  When summarization runs
  Then a short summary of decisions and action items appears
    And no network request is made
```

### F003-S2: Summary is editable in place

Covers: F003-R3

```gherkin
Scenario: The user edits the generated summary
  Given a generated summary
  When the user edits its text
  Then the edited text is retained in the summary section
```

### F003-S3: Summary is copied to the clipboard

Covers: F003-R4

```gherkin
Scenario: Copy places the summary on the clipboard
  Given a summary (edited or not)
  When the user triggers copy
  Then the current summary text is on the system clipboard
```

## Manual Verification Checklist

- [ ] (F003-R1) On a real Russian meeting transcript, the summary captures the
      actual decisions and action items well enough to edit rather than rewrite.
- [ ] (F003-R2) The summary section renders below the transcript.
- [ ] (F003-R5) A network monitor shows no traffic during summarization.
