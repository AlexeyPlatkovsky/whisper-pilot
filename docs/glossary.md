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
| **MFU** | The short meeting summary / follow-up the app generates — decisions and action items distilled from the transcript. |
| **Normalization (audio)** | Converting any input (audio or video) to 16 kHz mono PCM via ffmpeg, the single form the models consume. |
| **Whisper** | OpenAI's open speech-recognition model family; WhisperPilot runs a quantized `large-v3-turbo` locally via whisper.cpp. |
| **large-v3-turbo** | A multilingual Whisper model with a reduced decoder — fast, strong on Russian; used for transcription. |
| **Metal** | Apple's GPU API; whisper.cpp and llama.cpp are built with Metal so inference runs on the Apple Silicon GPU. |
| **sherpa-onnx** | An on-device speech toolkit; WhisperPilot uses its speaker-segmentation and embedding models for diarization (M2). |
| **llama.cpp / Qwen2.5** | The local LLM stack for summarization (M3): llama.cpp runtime running a quantized Qwen2.5-Instruct model. |
| **ffmpeg** | External tool that extracts audio from video and resamples audio to the normalized form. |
| **Full-file (batch)** | Transcribing an entire file at once with no real-time constraint, as opposed to live/streaming. |
