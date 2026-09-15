# WhisperPilot

![Version](https://img.shields.io/badge/version-1.20.0-6f55ff?style=flat-square)
![macOS](https://img.shields.io/badge/platform-macOS%2013%2B-111827?style=flat-square&logo=apple)
![Local-first](https://img.shields.io/badge/privacy-local--first-0f766e?style=flat-square)
![License](https://img.shields.io/badge/license-Apache--2.0-2563eb?style=flat-square)

WhisperPilot is a macOS app for local transcription. It turns files, live
system audio, and microphone recordings into editable transcripts while keeping
the core transcription and AI workflows on your Mac.

See the [version roadmap](ROADMAP.md) for the planned `v1.30`, `v1.40`, and
`v1.50` milestones.

## Three ways to work

### Transcription

Import an audio or video file, transcribe it locally, edit the result, identify
speakers, create structured notes, and export the finished text.

![Transcription workspace](images/transcription.jpg)

### Meeting

Listen to system audio during a live meeting, follow the transcript as it
arrives, and optionally show a paired translation. Completed sessions remain
in the local library for later editing, notes, and export.

![Meeting workspace with translation](images/meeting%20with%20transpation.jpg)

### Recorder

Capture your microphone into durable local audio, continue a recording later,
review the transcript as it settles, polish it in place, and export either text
or WAV audio.

![Recorder workspace](images/records.jpg)

## Local models and settings

Choose the local transcription and text models that fit your hardware. The app
uses Whisper large-v3-turbo by default and also supports Qwen3-ASR 1.7B Q8_0
for Transcription, Meeting, and Recorder. Appearance and status colors are
configured in Settings.

![Model settings](images/settings%20models.jpg)

![Appearance settings](images/settings%20colors.jpg)

## Requirements

- Apple Silicon Mac running macOS 13 or later
- [`ffmpeg`](https://ffmpeg.org/) on `PATH`:

  ```sh
  brew install ffmpeg
  ```

## Run from source

WhisperPilot is not distributed as a packaged download yet.

```sh
npm install
npm run tauri:dev
```

The first run compiles the Metal-accelerated local Whisper engine. See the
[developer guide](docs/development.md) for setup, validation, and packaging.

## Build

```sh
npx tauri build
```

The DMG is written under `src-tauri/target/release/bundle/dmg/`.

## Privacy

Your transcripts, Recorder audio, model files, and local AI work stay on your
Mac. See the [privacy boundary](docs/architecture/desktop.md#security-and-privacy)
for the complete technical description.

## License

[Apache License 2.0](LICENSE)
