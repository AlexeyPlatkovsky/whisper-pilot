# F001 File Transcription — Scenarios

## Scenarios

### F001-S1: Transcribe an audio file into segments

Covers: F001-R3

```gherkin
Scenario: Audio file produces timestamped segments
  Given a local audio file of clear speech
    And the Whisper model and ffmpeg are available
  When the user adds the file
  Then the transcript shows one or more non-empty segments
    And each segment has an ordered, non-degenerate start/end timestamp
```

### F001-S2: Video input is transcribed via extracted audio

Covers: F001-R2

```gherkin
Scenario: Video file is normalized through ffmpeg
  Given a local video file with an audio track
  When the user adds the file
  Then ffmpeg extracts and resamples the audio to 16 kHz mono
    And the transcript is produced from that audio
    And the temporary WAV is deleted afterward
```

### F001-S3: Edit a segment and save

Covers: F001-R4, F001-R5

```gherkin
Scenario: Edited transcript is saved
  Given a completed transcript
  When the user edits a segment's text
    And clicks Save and chooses a destination
  Then the saved file contains the edited text in segment order
```

### F001-S4: ffmpeg missing is reported, not fatal

Covers: F001-R6

```gherkin
Scenario: Missing ffmpeg surfaces an actionable error
  Given ffmpeg is not on PATH
  When the user adds a file
  Then an error banner states that ffmpeg is required
    And the app remains usable
```

### F001-S5: Missing model is reported

Covers: F001-R6

```gherkin
Scenario: Missing model surfaces an actionable error
  Given no Whisper model is present at the resolved path
  When the user adds a file
  Then an error banner states the model was not found and how to set its path
```

## Manual Verification Checklist

- [ ] (F001-R3) Russian audio transcribes accurately and in well-formed
      sentences — materially better than the live transcriber on similar audio.
- [ ] (F001-R1) The file picker filters to audio/video and cancelling is a no-op.
- [ ] (design: states) Empty, loading (named file), and error states each render
      as specified.
- [ ] (F001-R5) A saved file reopens with the edited text intact.
