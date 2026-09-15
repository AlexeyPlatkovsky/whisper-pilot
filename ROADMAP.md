# WhisperPilot Roadmap

WhisperPilot evolves through capability-focused releases. This roadmap uses
versions rather than dates: each milestone ships when its workflow is complete,
reliable, and ready for everyday use.

```mermaid
flowchart LR
    V130["<b>v1.30</b><br/>Cloud Providers<br/><small>Production-ready cloud transcription</small>"]
    V140["<b>v1.40</b><br/>Multi-file Transcription<br/><small>Several sources, one continuous transcript</small>"]
    V150["<b>v1.50</b><br/>Meeting Audio & Microphone<br/><small>Retained audio, optional mic, and retranscription</small>"]

    V130 --> V140 --> V150

    classDef cloud fill:#6f55ff,color:#ffffff,stroke:#4f3dcc,stroke-width:2px;
    classDef files fill:#0f9f8f,color:#ffffff,stroke:#08776b,stroke-width:2px;
    classDef meeting fill:#e59b2f,color:#17212b,stroke:#b97218,stroke-width:2px;
    class V130 cloud;
    class V140 files;
    class V150 meeting;
```

## v1.30 — Cloud Providers

Turn the existing cloud groundwork into a complete, production-ready workflow:
polished provider setup, secure credential handling, predictable lifecycle and
error recovery, and a consistent Meeting experience across local and cloud
transcription.

## v1.40 — Multi-file Transcription

Allow one Transcription to contain several audio or video files. Sources will be
processed in an explicit order and combined into one coherent transcript while
retaining enough source boundaries to review and manage the result reliably.

## v1.50 — Meeting Audio & Microphone

Add an optional microphone switch when starting a Meeting. A session will be
able to capture both system audio and the user's own microphone, while keeping
system-audio-only capture as the default. Meeting audio will be retained as a
durable source, with compact playback and WAV export presented alongside the
source details, as in Recorder.

A dedicated header action near the Meeting source and MFU controls will rerun
transcription from the retained original audio. Clearing a Meeting will remove
the transcript, MFU, translations, and its retained audio after confirmation.

For implementation-level sequencing and release criteria, see the
[internal roadmap](docs/roadmap.md).
