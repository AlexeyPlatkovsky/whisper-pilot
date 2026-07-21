# F002 Speaker Diarization — Scenarios

## Scenarios

### F002-S1: Multi-speaker file yields consistent speaker turns

Covers: F002-R1

```gherkin
Scenario: Diarization produces ordered speaker turns
  Given a local file with two distinct speakers
    And the sherpa-onnx models are available
  When the file is diarized
  Then ordered speaker turns covering the audio are produced
    And the same voice maps to the same speaker label throughout the file
```

### F002-S2: Segment is attributed to the max-overlap speaker

Covers: F002-R2

```gherkin
Scenario: Overlap assignment with deterministic tie-breaking
  Given a transcription segment spanning two speaker turns
  When the merge assigns a speaker
  Then the segment takes the speaker whose turns overlap it most
    And equal overlap resolves the same way every run
```

### F002-S3: Renaming a speaker updates all their segments and persists

Covers: F002-R4

```gherkin
Scenario: Speaker rename applies everywhere and survives save
  Given a per-speaker transcript with "Спикер 1" on several segments
  When the user renames "Спикер 1" to "Анна"
  Then every segment attributed to that speaker shows "Анна"
    And a saved transcript carries the "Анна" label
```

### F002-S4: Speaker count is provided or auto-detected

Covers: F002-R5

```gherkin
Scenario: Provided count is honored; otherwise detected
  Given a file to diarize
  When the user provides a speaker count of N
  Then exactly N speakers are used
  When the user provides no count
  Then a plausible speaker count is detected automatically
```

## Manual Verification Checklist

- [ ] (F002-R1) On a real 2–3 speaker Russian recording, turns align with who is
      actually speaking on spot checks.
- [ ] (F002-R3) Chat rendering distinguishes speakers by label (not color alone)
      and keeps timestamps and inline editing.
- [ ] (F002-R2) Segments spanning a speaker change are attributed sensibly.
- [ ] (compat) A file transcribed without diarization still renders as in M1.
