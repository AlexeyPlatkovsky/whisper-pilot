import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Segment {
  start_ms: number;
  end_ms: number;
  text: string;
  speaker_id?: number;
}

export interface MeetingSummary {
  id: number;
  title: string;
  created_at_ms: number;
  duration_ms?: number;
  status: string;
}

export interface MeetingNotes {
  meeting_id: number;
  summary: string;
  decisions: string;
  action_items: string;
  open_questions: string;
  participants: string;
}

export interface Meeting {
  id: number;
  title: string;
  source_path?: string;
  source_name?: string;
  created_at_ms: number;
  duration_ms?: number;
  language: string;
  status: string;
  segments: Segment[];
  notes?: MeetingNotes;
}

export function createMeeting(): Promise<Meeting> {
  return invoke<Meeting>("create_meeting");
}

export function listMeetings(): Promise<MeetingSummary[]> {
  return invoke<MeetingSummary[]>("list_meetings");
}

export function openMeeting(id: number): Promise<Meeting> {
  return invoke<Meeting>("open_meeting", { id });
}

export function renameMeeting(id: number, title: string): Promise<Meeting> {
  return invoke<Meeting>("rename_meeting", { id, title });
}

export function deleteMeeting(id: number): Promise<void> {
  return invoke<void>("delete_meeting", { id });
}

export function openFileDialog(): Promise<string | null> {
  return invoke<string | null>("open_file_dialog");
}

/** Attach a source file to a meeting, or clear it when `path` is null. */
export function setMeetingSource(
  id: number,
  path: string | null,
): Promise<Meeting> {
  return invoke<Meeting>("set_meeting_source", { id, path });
}

export interface TranscribeMeetingResult {
  meeting: Meeting;
  diarization_warning?: string;
}

/**
 * Transcribe the meeting's attached file and persist the result. When
 * diarization was attempted but its active model was missing or corrupt, the
 * transcription still succeeds with plain, speaker-less segments and
 * `diarization_warning` carries a short message to show the user.
 */
export function transcribeMeeting(
  id: number,
): Promise<TranscribeMeetingResult> {
  return invoke<TranscribeMeetingResult>("transcribe_meeting", { id });
}

export function generateNotes(id: number): Promise<Meeting> {
  return invoke<Meeting>("generate_notes", { id });
}

export function saveTextDialog(
  content: string,
  defaultName: string,
): Promise<string | null> {
  return invoke<string | null>("save_text_dialog", { content, defaultName });
}

export interface Settings {
  theme: string;
  ui_language: string;
  active_model_transcription?: string;
  active_model_diarization: string;
  active_model_llm?: string;
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function setSetting(key: string, value: string): Promise<Settings> {
  return invoke<Settings>("set_setting", { key, value });
}

export interface TaskModel {
  id: string;
  task: string;
  label: string;
  downloaded: boolean;
  size_bytes: number;
  recommended: boolean;
}

/**
 * Which part of a download `fraction` describes. Hashing a multi-hundred-
 * megabyte model runs on well after its last byte arrives, so verification is
 * reported as its own stage rather than as a still-downloading full bar.
 */
export type ModelDownloadStage = "downloading" | "verifying";

export interface ModelDownloadProgress {
  id: string;
  fraction: number;
  stage: ModelDownloadStage;
}

export function listTaskModels(): Promise<TaskModel[]> {
  return invoke<TaskModel[]>("list_task_models");
}

export function downloadModel(id: string): Promise<void> {
  return invoke<void>("download_model", { id });
}

export function deleteModel(id: string): Promise<void> {
  return invoke<void>("delete_model", { id });
}

export function onModelDownloadProgress(
  handler: (progress: ModelDownloadProgress) => void,
): Promise<UnlistenFn> {
  return listen<ModelDownloadProgress>("model_download_progress", (event) =>
    handler(event.payload),
  );
}

export interface TranscriptionPhase {
  id: number;
  phase: "diarizing";
}

/** Fired once a run moves from transcribing into diarizing its samples. */
export function onTranscriptionPhase(
  handler: (phase: TranscriptionPhase) => void,
): Promise<UnlistenFn> {
  return listen<TranscriptionPhase>("transcription_phase", (event) =>
    handler(event.payload),
  );
}

export interface StreamingSessionSummary {
  id: number;
  title: string;
  created_at_ms: number;
  updated_at_ms: number;
  status: string;
}

export interface StreamingWindow {
  window_index: number;
  start_ms: number;
  end_ms: number;
  text: string;
  language: string;
  outcome_ok: boolean;
}

export interface StreamingSession {
  id: number;
  title: string;
  created_at_ms: number;
  updated_at_ms: number;
  status: string;
  windows: StreamingWindow[];
}

export function listStreamingSessions(): Promise<StreamingSessionSummary[]> {
  return invoke<StreamingSessionSummary[]>("list_streaming_sessions");
}

export function openStreamingSession(id: number): Promise<StreamingSession> {
  return invoke<StreamingSession>("open_streaming_session", { id });
}

export function renameStreamingSession(
  id: number,
  title: string,
): Promise<StreamingSession> {
  return invoke<StreamingSession>("rename_streaming_session", { id, title });
}

export function deleteStreamingSession(id: number): Promise<void> {
  return invoke<void>("delete_streaming_session", { id });
}

/** Starts capture + rolling-window decode; returns once capture has begun. */
export function startStreamingSession(): Promise<StreamingSessionSummary> {
  return invoke<StreamingSessionSummary>("start_streaming_session");
}

export function stopStreamingSession(): Promise<void> {
  return invoke<void>("stop_streaming_session");
}

/** One decoded window, live — whether it succeeded or fail-open-skipped. */
export function onStreamingWindow(
  handler: (window: StreamingWindow & { session_id: number }) => void,
): Promise<UnlistenFn> {
  return listen<StreamingWindow & { session_id: number }>(
    "streaming_window",
    (event) => handler(event.payload),
  );
}

export interface StreamingSources {
  session_id: number;
  mic: boolean;
  system_audio: boolean;
}

/** Fired once, right after a session starts, naming which source(s) came up. */
export function onStreamingSources(
  handler: (sources: StreamingSources) => void,
): Promise<UnlistenFn> {
  return listen<StreamingSources>("streaming_sources", (event) =>
    handler(event.payload),
  );
}

/** Fired once the session's decode loop has fully ended (after Stop). */
export function onStreamingSessionEnded(
  handler: (payload: { session_id: number }) => void,
): Promise<UnlistenFn> {
  return listen<{ session_id: number }>("streaming_session_ended", (event) =>
    handler(event.payload),
  );
}
