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
editable transcript plus structured MFU — kept in a local library you
can reopen and edit. Local processing remains on-device and private, with
accuracy prioritized over speed because batch processing is offline.

For live audio — a meeting in progress, your own dictated thoughts, or audio
playing in the background — **Meeting** (ADR-014) gives a near-real-time,
plain-text transcript you can copy or export as it happens, still entirely
on-device. It trades a small latency budget for immediacy; it does not replace
Transcription's batch-accuracy pipeline, which is unchanged.

The **Recorder** workspace captures the user's microphone as a durable
voice note. It retains both local audio and an editable live transcript, supports
playback and WAV export, and can be controlled by a global shortcut without
turning a hidden webview into the capture authority. Its storage contract is
defined in ADR-017.

## Scope

### In scope

- Transcribing local **audio and video** files (audio extracted from video),
  **one file at a time**.
- **Offline, on-device** transcription, speaker separation, and detail generation.
- **Russian** as the primary language the app is tuned and validated for; the
  transcription language itself is always **auto-detected** from the audio,
  never chosen (ADR-012). English added afterward as a focus.
- A **speaker-attributed**, editable transcript.
- Structured **MFU** generated locally — summary, key decisions,
  action items, open questions, participants — editable and copyable.
- A persisted **library** of **transcriptions** (one item = one source file):
  reopen, rename, delete, with edits **auto-saved** locally.
- **Export** of a Transcription (transcript and/or MFU) to Markdown and plain text.
- A **Settings** screen: **AI models** (download/delete the model each task needs;
  at release, choose an Active model among several per task), **Appearance**
  (light / dark / system themes; more themes at release), **App language** (the UI
  language — English by default, more languages at release), and **Update app**
  (release only).
- One language **setting** — the **app UI language** (English by default). The
  **transcription language** is not a setting at all: Whisper detects it per run
  and the transcription records what was detected.
- **Meeting** (ADR-014): live, near-real-time transcription of system audio
  for a Meeting (the microphone is excluded) — plain, unattributed
  running text (no speaker separation), multi-language including mixed-language input
  within one session, with a roughly 5–10s latency budget. A separate,
  additive capability from Transcription; it does not use or affect
  Transcription's batch pipeline. No raw audio is retained for a Meeting, so it
  cannot later be re-transcribed with a different model.
- **Meeting live translation** (ADR-015): a Meeting transcript
  can be translated into English or Russian as it is captured, shown beside
  the original, using the same local summarization model — no new model and
  no cloud service. Meeting only; Transcription transcripts are not translated.
- **Cloud Meeting BYOK:** before starting a
  Meeting, users can select Local or Cloud transcription. Cloud
  exposes a fixed catalog — Deepgram Nova-3, AssemblyAI Universal-3.5 Pro, and
  OpenAI GPT Transcribe — and stores user-provided API keys only in macOS
  Keychain, after the provider verifies the key and model access without
  sending captured audio. Selecting Cloud always states that live audio would
  leave the device and be billed by the selected provider. Audio begins only
  after a TLS provider connection and OpenAI session setup succeed; finalized
  turns are retained locally. A
  provider/network failure stops capture with a retryable message and never
  falls back to Local.
- **Recorder:** microphone-only live capture as a third workspace,
  with Russian, English, and mixed-language transcription, retained app-owned
  audio, editable persisted text, playback, WAV export, interrupted-session
  recovery, and a configurable global Start/Stop shortcut. Recorder, Meeting,
  and active Transcription processing share one mutually exclusive transcription
  resource; Recorder never mixes microphone and system audio.

### Out of scope

- Cloud summarization and cloud storage. Cloud Meeting transcribes the live
  audio stream only; API keys never leave macOS Keychain except in the
  authenticated provider connection.
- Translation between languages, other than Meeting live translation
  into English or Russian (ADR-015). Transcription transcripts are not
  translated, and no other language pair is offered.
- Real-name speaker identification (voice enrollment); speakers are generic
  labels the user renames. Reassigning or merging speakers is deferred, so a
  misattributed segment cannot yet be corrected.
- Batch/queued processing of many files at once.
- In-app playback of Transcription source files or Meeting audio. Recorder playback
  is limited to the audio owned by its saved voice-note sessions.
- Multi-user collaboration, public distribution, notarization.

## Non-Goals

- Transcription will not become a real-time transcriber. Its accuracy comes
  specifically from full-file, batch processing (ADR-002), and that does not
  change. Meeting (ADR-014) is a separate, additive capability with its own
  quality-over-latency priority — it does not replace or dilute Transcription's
  batch-accuracy approach.
- It will not depend on any network service for its **core processing**:
  transcription (Transcription or Meeting) and MFU detail generation make no network
  calls. The only network use is downloading models (and, at release, app
  updates).

## Principles

- **Accuracy over speed.** Offline batch means the largest models and full-file
  context are affordable; use them.
- **Local-first.** No audio, transcript, or summary leaves the device;
  transcription and MFU generation run with no network access. Model downloads
  (and release updates) are the only networked step.
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
