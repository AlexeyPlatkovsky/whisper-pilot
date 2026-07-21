# Idea

## Problem

People with recordings of meetings, calls, and interviews — often in Russian —
need an accurate written record they can read, correct, and summarize. Live
transcribers fight a real-time budget and produce fragmented, low-accuracy text.
Cloud transcription raises privacy concerns for internal meetings and requires
connectivity. There is no simple, offline, on-device tool that turns a local
audio or video file into an accurate, speaker-attributed transcript plus a short
summary.

## Users

- **Meeting participant / organizer** — has a recording of a call and needs an
  accurate transcript and a short summary of decisions and action items.
- **Analyst / researcher** — transcribes interviews or discussions and needs the
  text attributed by speaker so it can be quoted and reviewed.
- **Russian-speaking professional** — needs high-quality Russian transcription
  that cloud English-first tools handle poorly.

## Value Proposition

Drop in an audio or video file and get back an accurate, speaker-labelled,
editable transcript and a short summary — entirely on-device, no cloud, with
accuracy prioritized over speed because processing is offline and batch.

## Scope

### In scope

- Transcribing local **audio and video** files (audio extracted from video).
- **Offline, on-device** transcription, speaker separation, and summarization.
- **Russian** as the primary language; English added afterward.
- A **speaker-attributed**, editable transcript that can be saved to a file.
- A short **summary / MFU** generated locally, editable and copyable.

### Out of scope

- Live / microphone / system-audio capture (that is a different product).
- Cloud transcription or summarization.
- Translation between languages.
- Real-name speaker identification (voice enrollment); speakers are generic
  labels the user renames.
- Multi-user collaboration, public distribution, notarization.

## Non-Goals

- WhisperPilot will not become a real-time transcriber. Accuracy from full-file,
  batch processing is the whole point.
- It will not depend on any network service for its core function.

## Principles

- **Accuracy over speed.** Offline batch means the largest models and full-file
  context are affordable; use them.
- **Local-first.** No audio, transcript, or summary leaves the device.
- **Editable truth.** Machine output is a starting point; every surface
  (transcript, labels, summary) is user-editable before it is trusted.
- **One pipeline, many inputs.** Normalize audio and video through a single
  ingestion path rather than special-casing formats.

## Success Signals

- Russian transcription of clear meeting audio reads accurately and in
  well-formed sentences — materially better than a live/streaming transcriber on
  the same audio.
- Within a file, speaker turns are attributed consistently to distinct speakers
  (M2).
- The generated summary captures the meeting's decisions and action items
  faithfully enough that a user edits rather than rewrites it (M3).
- A user can go from "add file" to a saved, corrected transcript without leaving
  the app or the device.
