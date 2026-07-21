# WhisperPilot Product Specification

## Product Goal

WhisperPilot is a macOS desktop application that transcribes **local audio and
video files** into accurate, **speaker-attributed** transcripts and generates a
short summary. It is **offline and local-first**: transcription, speaker
separation, and summarization all run on-device. There is **no live capture and
no cloud**.

The primary transcription language is **Russian**; English is added afterward.
The purpose is accuracy and precision over speed — because processing is offline
and batch, the app can use the largest models and full-file context with no
real-time constraint.

Target: Apple Silicon Macs running macOS 13 or later.

## Core Experience

1. Click **Add file** and choose an audio or video file from the system.
2. If the file is video, its audio is extracted automatically; audio files are
   used as-is.
3. The file is transcribed into a timestamped transcript.
4. The transcript is shown as an **editable, per-speaker chat**; edits can be
   saved to a file.
5. A short **summary / MFU** is generated in a separate section below the
   transcript, editable and copyable to the clipboard.

WhisperPilot never streams or captures live audio. It processes only
user-selected files.

## Delivery Milestones

Delivered as independently runnable milestones. A later milestone may extend or
replace earlier behavior.

| Milestone | Outcome | Status |
|---|---|---|
| M1 | Add file → extract audio → accurate Russian transcript → editable → save | **Done** |
| M2 | Speaker separation ("by roles") — per-speaker chat with editable labels | Planned |
| M3 | Local-LLM summary / MFU section — editable, copy to clipboard | Planned |

## Engines

- **Transcription:** Whisper `large-v3-turbo` (multilingual, quantized) via
  whisper-rs on Metal. Full-file, beam search.
- **Audio ingestion:** ffmpeg (extracts audio from video and resamples audio
  identically to 16 kHz mono).
- **Diarization (M2):** sherpa-onnx speaker segmentation + embedding models,
  fully local.
- **Summarization (M3):** llama.cpp running Qwen2.5-Instruct on Metal, fully
  local; strong Russian summarization.

## Boundaries

The product excludes: live/microphone/system-audio capture, cloud transcription,
translation, real-time output, multi-user/collaboration, and public distribution
or notarization. English transcription follows Russian. Real-name speaker
identification (voice enrollment) is out of scope; speakers are generic labels
the user renames.

## Acceptance Intent

- Russian transcription of clear meeting audio reads accurately and in
  well-formed sentences, materially better than a live/streaming transcriber.
- Speaker turns are attributed consistently within a file (M2).
- The summary captures decisions and action items faithfully and is editable
  before use (M3).
- Rust and React automated tests, build, lint, and format pass.
