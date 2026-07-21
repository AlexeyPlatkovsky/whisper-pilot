# WhisperPilot

WhisperPilot is an offline macOS app for turning local **audio and video
recordings** — meetings, calls, interviews — into accurate, editable
transcripts. Drop in a file and get back timestamped text you can correct and
save, entirely **on-device**: no live capture, no cloud, no network access
during processing.

Built for accuracy over speed: because processing runs offline in batch, it
uses full-file context and larger models than a real-time transcriber could
afford. Russian is the primary language today, with English and further
languages planned.

## Features

- **Local file transcription** — pick any local audio or video file and
  transcribe it end-to-end on-device.
- **Accurate Russian transcription** — full-file Whisper decoding (Metal
  acceleration) tuned for quality over real-time speed.
- **Editable, timestamped transcript** — every segment shows its start time
  and can be corrected in place.
- **Save to a text file** — export the current (edited) transcript whenever
  you're done.
- **Clear error handling** — missing dependencies or models surface as
  readable messages instead of crashes.

WhisperPilot is pre-1.0 and under active development. Planned next: a
persisted meeting library, speaker-attributed transcripts (colored
per-speaker chat), Markdown/plain-text export, and local AI-generated meeting
notes.

## Requirements

- macOS on Apple Silicon (macOS 13 or later)
- [`ffmpeg`](https://ffmpeg.org/) installed and available on your `PATH`
  (`brew install ffmpeg`)

## Running WhisperPilot

WhisperPilot isn't distributed as a packaged download yet — for now, run it
from source:

```sh
npm install
npm run tauri:dev
```

The first run compiles the local Whisper engine (Metal-accelerated), so it
takes longer than subsequent launches. See
[`docs/development.md`](docs/development.md) for the full developer guide.

## License

Apache License 2.0 — see [`LICENSE`](LICENSE).
