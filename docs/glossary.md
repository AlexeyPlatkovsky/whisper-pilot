# Glossary

Domain vocabulary for WhisperPilot. Register here any term whose meaning is not
obvious from general software knowledge.

| Term | Meaning |
| --- | --- |
| **Transcription** | The file-based workspace and one persisted source-file item. It converts speech audio into written text offline and full-file. Legacy implementation identifiers still use `Meeting`. |
| **Diarization** | Determining *who spoke when* — partitioning audio into speaker turns. Distinct from transcription (what was said). |
| **Speaker turn** | A contiguous time range attributed to one speaker, produced by diarization: `{ start_ms, end_ms, speaker }`. |
| **Segment** | A transcript unit produced by Whisper: `{ start_ms, end_ms, text }`. In M2 it also carries a speaker id. |
| **Merge (turn↔segment)** | Assigning each transcription segment a speaker by time-overlap with diarization turns. |
| **MFU** | The structured follow-up the app generates from a transcript: summary, key decisions, action items, open questions, participants (ADR-009). |
| **Action item** | A follow-up task in MFU, carrying an owner and a task description. |
| **Meeting** | The live system-audio workspace: a persisted, near-real-time transcript with optional translation and MFU. Legacy implementation identifiers still use `Streaming` or `StreamingSession`. |
| **Library** | A persisted local collection of Transcription, Meeting, or Recorder items, browsable and reopenable (ADR-008, ADR-014, ADR-017). |
| **MFU section** | The panel below the transcript holding generated MFU; empty by default (15% height) with a Create MFU button, populated at 30% with edit/copy/clear. |
| **Attached file** | The single source audio/video file bound to a Transcription item before processing; shown in the status bar with an × to detach. |
| **Auto-save** | Edits (transcript, speaker labels, MFU) persist to the library immediately, with no explicit save action. |
| **Source-missing** | State of a Transcription whose original file moved or was deleted: readable/editable, but re-transcribe is disabled. |
| **Normalization (audio)** | Converting any input (audio or video) to 16 kHz mono PCM via ffmpeg, the single form the models consume. |
| **Whisper** | OpenAI's open speech-recognition model family; WhisperPilot runs a quantized `large-v3-turbo` locally via whisper.cpp. |
| **large-v3-turbo** | A multilingual Whisper model with a reduced decoder — fast, strong on Russian; used for transcription. |
| **Metal** | Apple's GPU API; whisper.cpp and llama.cpp are built with Metal so inference runs on the Apple Silicon GPU. |
| **sherpa-onnx** | An on-device speech toolkit; WhisperPilot uses its speaker-segmentation and embedding models for diarization (M2). |
| **llama.cpp / local GGUF** | The shared local inference stack for MFU, translation, transcript polishing, and Qwen3-ASR 1.7B audio decoding. |
| **ffmpeg** | External tool that extracts audio from video and resamples audio to the normalized form. |
| **Settings** | The app-wide configuration screen (F005), opened from the header gear: AI models, Appearance, App language, and (release) Update app. |
| **Model catalog** | The fixed, app-defined list of the model(s) each task (transcription, diarization, MFU) needs; managed in Settings → AI models (download/delete/verify). Not user-extensible. |
| **Active model** | (Release) When a task has several downloaded models, the one selected (radio) for that task to use. |
| **Theme** | The app's visual scheme: Light, Dark, or System (follows the OS) in beta; extra named themes (each light + dark) at release. |
| **UI language** | The language of the app's interface (English by default; more at release) — distinct from the **transcription language**, which is auto-detected per run and never chosen (ADR-012). |
| **Transcription language** | The language detected from source audio and stored on the item. It is an output of a run, not an input (ADR-012). |
| **Full-file (batch)** | Transcribing an entire file at once with no real-time constraint, as opposed to live/streaming. |
| **Live translation** | Meeting's translation of the running transcript into a chosen target language while capture continues (ADR-015, ADR-016). It never changes the detected source text; it adds a translated column. |
| **Target language** | The language a Meeting transcript is translated into when Live Translation is on: English or Russian. It is distinct from the auto-detected transcription language (ADR-012, ADR-014). |
| **Window (translation unit)** | The unit Live Translation actually translates and persists — one `StreamingWindow`, not a paragraph or a sentence (ADR-016; a paragraph was the unit before this). Each window has its own translation, produced with the up-to-2 preceding windows' translations as rolling context. |
| **Paragraph (display/alignment unit)** | The unit the Meeting split view groups and aligns. A paragraph and its translated cell render at the same vertical position; internally it is built from legacy `StreamingWindow` records (ADR-016). |
