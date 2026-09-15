# Roadmap

Owns planned release capabilities and sequencing. The public version track is
shown in [`../ROADMAP.md`](../ROADMAP.md); feature/task breakdown and lifecycle
status live in TaskPilot (`WP-<n>`).

## Release stance

WhisperPilot uses capability-driven versions rather than calendar dates. The
current `1.20` line remains local-first and fully usable while later milestones
add optional workflows without replacing the existing local paths.

## v1.30 — Production-ready Cloud Providers

**Goal:** turn the existing Cloud Meeting groundwork into a complete supported
workflow rather than presenting partial provider integration as finished.

**Scope:**

- polished provider and model selection before Meeting capture;
- secure credential setup, validation, replacement, and removal;
- deterministic start, stop, reconnect, timeout, and error behavior;
- consistent transcript persistence and user-visible recovery across supported
  providers;
- clear privacy and billing boundaries while preserving Local as a first-class
  option.

**Exit criteria:** supported cloud providers can be configured and used through
the complete Meeting lifecycle without silent fallback, lost final text, leaked
credentials, or provider-specific UI inconsistencies.

## v1.40 — Multi-file Transcription

**Goal:** let one Transcription combine several audio or video sources into one
coherent result.

**Scope:**

- add, remove, and reorder multiple source files within one Transcription;
- process sources as one ordered transcription job;
- produce one continuous editable transcript and MFU result;
- retain source boundaries and monotonic timeline metadata for review, recovery,
  and export;
- make partial failure actionable without discarding completed sources.

**Exit criteria:** a user can create one Transcription from multiple ordered
files, run it end to end, reopen it, edit it, and export it as one coherent
artifact.

## v1.50 — Meeting Audio, Optional Microphone, and Retranscription

**Goal:** make Meeting capture durable and reusable while optionally recording
the user's own voice alongside system audio.

**Scope:**

- add a microphone on/off option to the Meeting start configuration;
- keep system-audio-only capture as the default;
- capture system audio and microphone input on one stable session timeline;
- retain the completed Meeting audio as a durable local source;
- show compact playback and a dedicated WAV export action alongside the audio
  source details, following the Recorder interaction pattern without increasing
  the header height;
- add a dedicated Retranscribe action near the source/mode and MFU controls in
  the Meeting header, rerunning recognition from the retained original audio;
- replace the current transcript atomically after successful retranscription,
  while preserving the prior transcript if recognition fails;
- make confirmed Clear remove the transcript, MFU, translations, and retained
  audio file together;
- expose the active input state clearly and preserve existing live translation,
  transcript, and error behavior;
- handle permission denial, device removal, and input failure without silently
  losing the system-audio stream.

**Exit criteria:** a user can explicitly start a Meeting with or without their
microphone; the resulting audio can be replayed and exported; transcription can
be regenerated from that audio without risking the existing result; confirmed
Clear removes all derived content and the retained audio; and permission or
device failures produce recoverable, source-specific feedback.

## Sequence

`v1.30` stabilizes the optional network path before the Transcription data model
expands in `v1.40`. The `v1.50` durable and mixed-input Meeting work follows so
its audio, retranscription, and lifecycle changes build on already stable
local/cloud Meeting behavior.
