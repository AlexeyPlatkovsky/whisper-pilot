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

export interface MeetingMfu {
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
  mfu?: MeetingMfu;
  /** `true` when an attached source file is no longer readable at its saved
   * path (moved or deleted). `false` when there is no source at all. */
  source_missing: boolean;
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

/** Auto-save an edited segment's text; `index` addresses the displayed
 * (speaker-coalesced) segment list. No explicit save action is required. */
export function updateSegment(
  id: number,
  index: number,
  text: string,
): Promise<Meeting> {
  return invoke<Meeting>("update_segment", { id, index, text });
}

/** Auto-save the meeting MFU fields as the user edits them. */
export function updateMfu(mfu: MeetingMfu): Promise<Meeting> {
  return invoke<Meeting>("update_mfu", { mfu });
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

/**
 * Re-run speaker identification alone on an already-transcribed meeting,
 * leaving the transcript text untouched. Requires an active diarization
 * model and a readable source file.
 */
export function diarizeMeeting(id: number): Promise<TranscribeMeetingResult> {
  return invoke<TranscribeMeetingResult>("diarize_meeting", { id });
}

export function generateMfu(id: number): Promise<Meeting> {
  return invoke<Meeting>("generate_mfu", { id });
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
  export_file_type: string;
  /** JSON mapping of status key → opaque #RRGGBB color (WP-88); absent before
   * the setting is first saved. */
  status_colors?: string;
  /** Whether the Meeting screen's MFU panel is shown (WP-96); absent before
   * the setting is first saved — treat as `true` (the default). */
  mfu_panel_meeting?: boolean;
  /** Same as `mfu_panel_meeting`, for the Streaming screen (WP-96). */
  mfu_panel_streaming?: boolean;
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

export interface TranscriptionProgress {
  id: number;
  percent: number;
}

/** Fired with Whisper's 0–100 estimate while a Meeting is transcribing. */
export function onTranscriptionProgress(
  handler: (progress: TranscriptionProgress) => void,
): Promise<UnlistenFn> {
  return listen<TranscriptionProgress>("transcription_progress", (event) =>
    handler(event.payload),
  );
}

export interface StreamingSessionSummary {
  id: number;
  title: string;
  created_at_ms: number;
  updated_at_ms: number;
  status: string;
  /** Whether Live Translation was left on for this session (WP-101).
   * Unlike the target language (WP-99), this survives reopening the session
   * and an app restart. */
  translation_enabled: boolean;
}

export interface StreamingWindow {
  window_index: number;
  start_ms: number;
  end_ms: number;
  text: string;
  language: string;
  outcome_ok: boolean;
}

export interface StreamingMfu {
  summary: string;
  decisions: string;
  action_items: string;
  open_questions: string;
  participants: string;
}

export interface StreamingSession {
  id: number;
  title: string;
  created_at_ms: number;
  updated_at_ms: number;
  status: string;
  windows: StreamingWindow[];
  mfu?: StreamingMfu;
  prettified_text?: string;
  /** See `StreamingSessionSummary.translation_enabled` (WP-101). */
  translation_enabled: boolean;
}

export function generateStreamingMfu(id: number): Promise<StreamingSession> {
  return invoke<StreamingSession>("generate_streaming_mfu", { id });
}

/** Returns the cleaned text for review — does not persist it. */
export function generateStreamingPrettify(id: number): Promise<string> {
  return invoke<string>("generate_streaming_prettify", { id });
}

export function acceptStreamingPrettify(
  id: number,
  text: string,
): Promise<StreamingSession> {
  return invoke<StreamingSession>("accept_streaming_prettify", { id, text });
}

/** Removes an accepted prettification and restores the raw transcript view. */
export function revertStreamingPrettify(id: number): Promise<StreamingSession> {
  return invoke<StreamingSession>("revert_streaming_prettify", { id });
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

/** Creates a stopped session record. It does not start audio capture. */
export function createStreamingSession(): Promise<StreamingSessionSummary> {
  return invoke<StreamingSessionSummary>("create_streaming_session");
}

/** Starts capture + rolling-window decode; returns once capture has begun.
 * Pass `sessionId` to resume a previously-stopped session (window numbering
 * continues where it left off) instead of starting a brand-new one. */
export function startStreamingSession(
  sessionId?: number,
): Promise<StreamingSessionSummary> {
  return invoke<StreamingSessionSummary>("start_streaming_session", {
    sessionId,
  });
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

/** Target languages Streaming paragraph translation supports (WP-92). */
export type StreamingTranslationTargetLanguage = "en" | "ru";

/**
 * Translates one Streaming window into `targetLanguage` using the active
 * local LLM and persists the result, keyed by `(sessionId, windowIndex,
 * targetLanguage)`. Single-flight in the core: a second concurrent call
 * rejects with a distinct, retryable error. `context` (WP-100/WP-103) is
 * whatever rolling context the caller assembles, threaded through unchanged
 * as ephemeral prompt context.
 */
export function translateStreamingWindow(
  sessionId: number,
  windowIndex: number,
  targetLanguage: StreamingTranslationTargetLanguage,
  text: string,
  context?: string,
): Promise<string> {
  return invoke<string>("translate_streaming_window", {
    sessionId,
    windowIndex,
    targetLanguage,
    text,
    context,
  });
}

/** One persisted window translation, as returned by
 * `listStreamingTranslations`. `source_text` is the window's own text the
 * translation was made from — compare it against that window's *current*
 * text to detect a stale row (the window's text changed since, e.g. a
 * fail-open retry). */
export interface StreamingTranslationRow {
  window_index: number;
  source_text: string;
  translated_text: string;
}

/**
 * All persisted translations for `sessionId` and `targetLanguage` (WP-93) —
 * the read counterpart to `translateStreamingWindow`, letting the caller
 * reuse an already-translated window instead of re-running the model.
 */
export function listStreamingTranslations(
  sessionId: number,
  targetLanguage: StreamingTranslationTargetLanguage,
): Promise<StreamingTranslationRow[]> {
  return invoke<StreamingTranslationRow[]>("list_streaming_translations", {
    sessionId,
    targetLanguage,
  });
}

/**
 * Persists the Live Translation on/off choice for one session (WP-101).
 * Best-effort from the caller's perspective — matching WP-96's MFU-panel
 * toggle pattern, a rejected promise here should be swallowed rather than
 * surfaced as a blocking error or used to revert the switch.
 */
export function setStreamingTranslationEnabled(
  sessionId: number,
  enabled: boolean,
): Promise<void> {
  return invoke<void>("set_streaming_translation_enabled", {
    sessionId,
    enabled,
  });
}
