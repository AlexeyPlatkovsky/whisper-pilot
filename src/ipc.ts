import { invoke } from "@tauri-apps/api/core";

export interface Segment {
  start_ms: number;
  end_ms: number;
  text: string;
}

export interface TranscriptResult {
  file_name: string;
  segments: Segment[];
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
