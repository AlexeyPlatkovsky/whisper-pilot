# WhisperPilot

WhisperPilot is a macOS app for turning local **audio and video recordings** —
meetings, calls, interviews — and live Streaming sources into accurate,
editable transcripts. Local processing stays **on-device**. Streaming also has
an optional Cloud BYOK streaming mode for supported providers. It stores keys
in macOS Keychain and sends live audio only after the user explicitly selects
Cloud and the provider connection succeeds.

Meeting transcription runs offline in batch. Streaming and Recorder process
bounded live windows. Whisper large-v3-turbo remains the default across all
three modes; Qwen3-ASR 1.7B Q8_0 is an optional local GGUF model for all three.

## Features

- **Local file transcription** — pick any local audio or video file and
  transcribe it end-to-end on-device.
- **Recorder** — capture the default microphone into recoverable local audio,
  see stable live phrases, and start or stop from a global shortcut.
- **Selectable local ASR** — choose Whisper or Qwen3-ASR 1.7B Q8_0 once for
  Meeting, Streaming, and Recorder.
- **Accurate Russian transcription** — full-file Whisper decoding (Metal
  acceleration) tuned for quality over real-time speed. The spoken language is
  detected automatically; there is nothing to configure.
- **Editable transcripts** — Meeting and Streaming show segment start times;
  Recorder adds the same searchable library and header controls, in-place
  Prettify, and transcript-only Clear while retaining its local audio.
- **Save to a text file** — export the current (edited) transcript whenever
  you're done.
- **Clear error handling** — missing dependencies or models surface as
  readable messages instead of crashes.

WhisperPilot is under active development. Its persisted Meeting, Streaming,
and Recorder libraries support reopen/manage flows, recoverable Recorder audio,
editable transcripts, export, speaker attribution, and local AI processing.

## Requirements

- macOS on Apple Silicon (macOS 13 or later)
- [`ffmpeg`](https://ffmpeg.org/) installed and available on your `PATH`: 
```
brew install ffmpeg
```

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

## Build the app
```
npx tauri build
```

The output goes to `src-tauri/target/release/bundle/dmg/`

To be able to run the app in any Macbook, add app to verified list:
```
xattr -dr com.apple.quarantine /Applications/WhisperPilot.app
```

## License

Apache License 2.0 — see [`LICENSE`](LICENSE).
