# WhisperPilot Architecture

Technical architecture for the offline file-transcription app. Product scope is
owned by `docs/idea.md`; this document owns the layer map, pipeline, IPC
contract, models, and build notes.

## Layer Map

```
React UI (src/)  ──Tauri IPC──▶  Rust core (src-tauri/src/)
  App.tsx                          lib.rs        command registration + AppState
  ipc.ts                           audio.rs      ffmpeg normalize + WAV decode
  styles.css                       transcribe.rs whisper (Metal) full-file decode
                                   error.rs      AppError → serialized to JS
                                   [M2] diarize.rs   sherpa-onnx speaker turns
                                   [M3] summarize.rs llama.cpp summary / MFU
```

The Rust core does all heavy work; the React layer is a thin editor/viewer.
Blocking, CPU/GPU-heavy work (model load, transcription) runs on
`tokio::task::spawn_blocking` so IPC and UI stay responsive.

## Audio Ingestion (`audio.rs`)

Any input — audio or video — is normalized through **one** path: ffmpeg is
invoked as a subprocess to produce a temporary 16 kHz mono WAV
(`-vn -ac 1 -ar 16000`), which is then decoded with `hound` into f32 samples in
[-1, 1]. ffmpeg extracts audio from video and resamples audio identically, so no
branch on file type is needed. The temporary WAV is deleted after decoding.

ffmpeg is a required external dependency (system binary on PATH for M1; a bundled
sidecar is a later hardening step).

## Transcription (`transcribe.rs`)

whisper-rs with the `metal` feature; the context is created once and cached in
`AppState` (lazy load on first transcription). Decoding is **full-file** with
`SamplingStrategy::BeamSearch { beam_size: 5 }`, temperature fallback, and
`language = "ru"` by default. Output is a list of `Segment { start_ms, end_ms,
text }` (whisper timestamps are centiseconds → milliseconds). No VAD, no
streaming, no real-time constraint — accuracy is the only objective.

For M1 the model is the `large-v3-turbo` artifact already managed on disk;
override with `MFUPILOT_MODEL_PATH`. WhisperPilot will manage its own model
catalog in a later milestone.

## Diarization (M2, `diarize.rs`) — planned

sherpa-onnx speaker segmentation + embedding models produce speaker turns
(time ranges labelled by cluster). Turns are merged onto transcription segments
by time overlap to attribute each segment to a speaker. Labels are generic
("Спикер 1/2/…") and user-editable. Speaker count may be provided or
auto-detected.

## Summarization (M3, `summarize.rs`) — planned

llama.cpp running a quantized Qwen2.5-Instruct model on Metal produces a short
summary / MFU from the finalized transcript. Fully local; editable and copyable
in the UI.

## IPC Contract

| Command | Args | Returns | Milestone |
|---|---|---|---|
| `open_file_dialog` | — | `String?` (path) | M1 |
| `transcribe_file` | `path`, `language?` | `TranscriptResult { file_name, segments }` | M1 |
| `save_text_dialog` | `content`, `default_name?` | `String?` (written path) | M1 |
| `diarize_file` | `path` | speaker turns | M2 |
| `summarize` | `transcript` | summary text | M3 |

`Segment` is the shared transcript unit: `{ start_ms: u64, end_ms: u64, text:
String }`. Errors are `AppError` serialized to a human-readable string.

## Security And Privacy

Fully local. No network calls, no telemetry. The only file writes are the
temporary ffmpeg WAV (deleted after use) and the user-chosen save destination.
No `.env` or secret handling.

## Build Notes

- Tauri v2 + React 19 + TypeScript; Vite dev server on port 1420.
- `whisper-rs = { features = ["metal"] }` — compiles whisper.cpp with Metal;
  the shader library is embedded, so nothing extra ships alongside.
- ffmpeg must be on PATH (`which ffmpeg`).
- Run: `npm install`, then `npm run tauri:dev`.

## Ownership

| Concern | Owner |
|---|---|
| Command registration, app state, model cache | `src-tauri/src/lib.rs` |
| Audio normalize + decode | `src-tauri/src/audio.rs` |
| Whisper transcription | `src-tauri/src/transcribe.rs` |
| Error type | `src-tauri/src/error.rs` |
| UI, editing, save/copy | `src/App.tsx`, `src/ipc.ts` |
