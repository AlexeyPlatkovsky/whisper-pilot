import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Segment {
  start_ms: number;
  end_ms: number;
  text: string;
  speaker_id?: number;
}

export interface TranscriptResult {
  file_name: string;
  segments: Segment[];
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

export function openFileDialog(): Promise<string | null> {
  return invoke<string | null>("open_file_dialog");
}

export function transcribeFile(
  path: string,
  language = "ru",
): Promise<TranscriptResult> {
  return invoke<TranscriptResult>("transcribe_file", { path, language });
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
