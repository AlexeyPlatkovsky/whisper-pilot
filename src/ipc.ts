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
  active_model_diarization?: string;
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

export interface ModelDownloadProgress {
  id: string;
  fraction: number;
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
