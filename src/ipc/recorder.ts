import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type MicrophonePermissionStatus =
  "not_determined" | "denied" | "restricted" | "authorized" | "unavailable";

/** Reads macOS authorization state without opening a device or showing TCC. */
export function getMicrophonePermissionStatus(): Promise<MicrophonePermissionStatus> {
  return invoke<MicrophonePermissionStatus>("get_microphone_permission_status");
}

/** May show TCC only when called from an explicit Recorder user action. */
export function requestMicrophonePermission(): Promise<MicrophonePermissionStatus> {
  return invoke<MicrophonePermissionStatus>("request_microphone_permission");
}

export function showRecorderWorkspace(): Promise<void> {
  return invoke<void>("show_recorder_workspace");
}

export function onOpenRecorderWorkspace(
  handler: () => void,
): Promise<UnlistenFn> {
  return listen<void>("open_recorder_workspace", handler);
}

export type RecorderStatus =
  "recording" | "finalizing" | "completed" | "recoverable" | "delete_failed";

export interface RecorderSegment {
  id: number;
  session_id?: number;
  start_sample: number;
  end_sample: number;
  text: string;
  language: string;
}

export interface RecorderSession {
  id: number;
  title: string;
  created_at_ms: number;
  updated_at_ms: number;
  duration_ms: number;
  status: RecorderStatus;
  is_draft: boolean;
  sample_rate: number;
  recovery_reason?: string;
  audio_path: string;
  asr_model_id: string;
  asr_engine: string;
  asr_language: string;
  segments: RecorderSegment[];
  polished_text?: string;
}

export type RecorderSessionSummary = Omit<RecorderSession, "segments"> & {
  segments?: RecorderSegment[];
};

export function listRecorderSessions(): Promise<RecorderSessionSummary[]> {
  return invoke<RecorderSessionSummary[]>("list_recorder_sessions");
}

export function openRecorderSession(id: number): Promise<RecorderSession> {
  return invoke<RecorderSession>("open_recorder_session", { id });
}

export function renameRecorderSession(
  id: number,
  title: string,
): Promise<RecorderSession> {
  return invoke<RecorderSession>("rename_recorder_session", { id, title });
}

export function deleteRecorderSession(id: number): Promise<void> {
  return invoke<void>("delete_recorder_session", { id });
}

export function clearRecorderRecording(id: number): Promise<RecorderSession> {
  return invoke<RecorderSession>("clear_recorder_recording", { id });
}

export function createRecorderDraft(): Promise<RecorderSession> {
  return invoke<RecorderSession>("create_recorder_draft");
}

export function startRecorderSession(
  draftId?: number,
): Promise<RecorderSession> {
  return invoke<RecorderSession>("start_recorder_session", { draftId });
}

export function stopRecorderSession(): Promise<void> {
  return invoke<void>("stop_recorder_session");
}

export function recoverRecorderSession(id: number): Promise<RecorderSession> {
  return invoke<RecorderSession>("recover_recorder_session", { id });
}

export function updateRecorderSegment(
  sessionId: number,
  segmentId: number,
  text: string,
): Promise<RecorderSegment> {
  return invoke<RecorderSegment>("update_recorder_segment", {
    sessionId,
    segmentId,
    text,
  });
}

export function exportRecorderWav(id: number): Promise<string | null> {
  return invoke<string | null>("export_recorder_wav", { id });
}

/** Produces a review candidate and leaves the timestamped raw transcript unchanged. */
export function generateRecorderPolish(id: number): Promise<string> {
  return invoke<string>("generate_recorder_polish", { id });
}

export function acceptRecorderPolish(
  id: number,
  text: string,
): Promise<RecorderSession> {
  return invoke<RecorderSession>("accept_recorder_polish", { id, text });
}

export function revertRecorderPolish(id: number): Promise<RecorderSession> {
  return invoke<RecorderSession>("revert_recorder_polish", { id });
}

export function onRecorderSessionChanged(
  handler: (session: RecorderSession) => void,
): Promise<UnlistenFn> {
  return listen<RecorderSession>("recorder_session_changed", (event) =>
    handler(event.payload),
  );
}

export function onRecorderSegmentCommitted(
  handler: (segment: RecorderSegment & { session_id: number }) => void,
): Promise<UnlistenFn> {
  return listen<RecorderSegment & { session_id: number }>(
    "recorder_segment_committed",
    (event) => handler(event.payload),
  );
}

export interface RecorderPartial {
  session_id: number;
  revision: number;
  text: string;
}

export function onRecorderPartial(
  handler: (partial: RecorderPartial) => void,
): Promise<UnlistenFn> {
  return listen<RecorderPartial>("recorder_partial", (event) =>
    handler(event.payload),
  );
}

export function onRecorderError(
  handler: (error: { session_id?: number; message: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ session_id?: number; message: string }>(
    "recorder_error",
    (event) => handler(event.payload),
  );
}
