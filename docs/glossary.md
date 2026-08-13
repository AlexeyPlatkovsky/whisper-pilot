# Glossary

Domain vocabulary for WhisperPilot. Register here any term whose meaning is not
obvious from general software knowledge.

| Term | Meaning |
| --- | --- |
| **Transcription** | Converting speech audio into written text. In WhisperPilot, offline and full-file. |
| **Diarization** | Determining *who spoke when* — partitioning audio into speaker turns. Distinct from transcription (what was said). |
| **Speaker turn** | A contiguous time range attributed to one speaker, produced by diarization: `{ start_ms, end_ms, speaker }`. |
| **Segment** | A transcript unit produced by Whisper: `{ start_ms, end_ms, text }`. In M2 it also carries a speaker id. |
| **Merge (turn↔segment)** | Assigning each transcription segment a speaker by time-overlap with diarization turns. |
| **Meeting MFU / MFU** | The structured follow-up the app generates from the transcript: summary, key decisions, action items, open questions, participants (ADR-009). |
| **Action item** | A follow-up task in the meeting MFU, carrying an owner and a task description. |
| **Meeting** | One transcription of one source file — the core unit stored in the library and shown per row in the left list: metadata + attached file + segments + MFU MFU. (UI, data, and IPC all use "meeting".) |
| **Library** | The persisted local collection of meetings (SQLite), browsable and reopenable (ADR-008). |
| **MFU section** | The panel below the transcript holding the generated meeting MFU; empty by default (15% height) with a Create MFU button, populated at 30% with edit/copy/clear. |
| **Attached file** | The single source audio/video file bound to a meeting before transcription; shown in the status bar with an × to detach (MVP: one per meeting). |
| **Auto-save** | Edits (transcript, speaker labels, MFU) persist to the library immediately, with no explicit save action. |
| **Source-missing** | State of a meeting whose original file has moved or been deleted: readable/editable, but re-transcribe is disabled. |
| **Normalization (audio)** | Converting any input (audio or video) to 16 kHz mono PCM via ffmpeg, the single form the models consume. |
| **Whisper** | OpenAI's open speech-recognition model family; WhisperPilot runs a quantized `large-v3-turbo` locally via whisper.cpp. |
| **large-v3-turbo** | A multilingual Whisper model with a reduced decoder — fast, strong on Russian; used for transcription. |
| **Metal** | Apple's GPU API; whisper.cpp and llama.cpp are built with Metal so inference runs on the Apple Silicon GPU. |
| **sherpa-onnx** | An on-device speech toolkit; WhisperPilot uses its speaker-segmentation and embedding models for diarization (M2). |
| **llama.cpp / Qwen2.5** | The local LLM stack for summarization (M3): llama.cpp runtime running a quantized Qwen2.5-Instruct model. |
| **ffmpeg** | External tool that extracts audio from video and resamples audio to the normalized form. |
| **Settings** | The app-wide configuration screen (F005), opened from the header gear: AI models, Appearance, App language, and (release) Update app. |
| **Model catalog** | The fixed, app-defined list of the model(s) each task (transcription, diarization, MFU) needs; managed in Settings → AI models (download/delete/verify). Not user-extensible. |
| **Active model** | (Release) When a task has several downloaded models, the one selected (radio) for that task to use. |
| **Theme** | The app's visual scheme: Light, Dark, or System (follows the OS) in beta; extra named themes (each light + dark) at release. |
| **UI language** | The language of the app's interface (English by default; more at release) — distinct from the **transcription language**, which is auto-detected per run and never chosen (ADR-012). |
| **Transcription language** | The language Whisper detected in a meeting's audio, stored on the meeting. An output of a run, not an input to one; there is no way to force it (ADR-012). |
| **Full-file (batch)** | Transcribing an entire file at once with no real-time constraint, as opposed to live/streaming. |
