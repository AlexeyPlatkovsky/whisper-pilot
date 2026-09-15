# WhisperPilot Architecture

Technical boundaries and ownership for the Transcription, Meeting, Recorder,
and local-AI workspaces. Product scope belongs to [idea.md](idea.md); UI flows
and states belong to [design.md](design.md).

User-facing terminology is **Transcription** for the full-file workflow and
**Meeting** for live system-audio capture. Legacy Rust types, IPC commands,
database tables, filenames, and CSS classes may still use `Meeting` for the
file workflow and `Streaming`/`StreamingSession` for live capture. Code-formatted
legacy names always refer to implementation identifiers, not UI copy.

## Layer map

```text
React UI (src/)  ──Tauri IPC──▶  Rust core (src-tauri/src/)
  Transcription / Meeting /        commands/       domain IPC facades
  Recorder workspaces              audio.rs        decode and normalize
  settings, models, theming        state.rs        runtime ownership
  transcript and MFU rendering     stores          SQLite persistence
                                   ASR / LLM       local model runtimes
```

The Rust core owns persistence and heavy work. React is the workspace shell.
CPU/GPU work uses blocking tasks so IPC stays responsive; live capture has an
independent lifecycle while full-file Transcription runs to completion.

## Functional documents

| Area | Owns |
| --- | --- |
| [Transcription](architecture/transcription.md) | Full-file source lifecycle, persistence, decode, diarization |
| [Meeting](architecture/meeting.md) | System-audio capture, rolling decode, live persistence and UI |
| [Recorder](architecture/recorder.md) | Microphone capture, durable audio, continuation and storage |
| [Local AI](architecture/local-ai.md) | MFU, Prettify, translation and optional Cloud adapter |
| [Platform](architecture/platform.md) | Settings, model catalog, export and IPC contract |
| [Desktop, privacy and build](architecture/desktop.md) | Window integration, data boundaries, packaging and ownership |

Read the relevant functional document rather than treating this index as a
complete implementation reference.
