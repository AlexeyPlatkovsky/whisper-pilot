import { screen, waitFor } from "@testing-library/react";
import { expect, vi } from "vitest";
import * as ipc from "../ipc";
import type { Meeting, Segment, TranscribeMeetingResult } from "../ipc";

export const TRANSCRIPTION_DOWNLOADED = {
  id: "transcription",
  task: "transcription",
  label: "Whisper large-v3-turbo (Q8)",
  downloaded: true,
  size_bytes: 874_188_075,
  recommended: false,
};

export const TRANSCRIPTION_NOT_DOWNLOADED = {
  ...TRANSCRIPTION_DOWNLOADED,
  downloaded: false,
};

// The workspace is always backed by a real, persisted meeting. On an empty
// library the app seeds this one on mount; attaching a file and transcribing
// are separate, explicit steps that each return an updated meeting.
export const EMPTY_MEETING: Meeting = {
  id: 100,
  title: "New Meeting",
  created_at_ms: 0,
  language: "ru",
  status: "no_files",
  segments: [],
  source_missing: false,
};

export const ATTACHED_MEETING: Meeting = {
  ...EMPTY_MEETING,
  source_path: "/path/to/meeting.mp3",
  source_name: "meeting.mp3",
  status: "ready",
  segments: [],
};

export function transcribedMeeting(segments: Segment[]): Meeting {
  return {
    ...ATTACHED_MEETING,
    status: "finished",
    duration_ms: segments.at(-1)?.end_ms ?? 0,
    segments,
  };
}

export function transcribeResult(
  meeting: Meeting,
  diarizationWarning?: string,
): TranscribeMeetingResult {
  return { meeting, diarization_warning: diarizationWarning };
}

export const HELLO_SEGMENT: Segment = {
  start_ms: 0,
  end_ms: 1000,
  text: "Hello",
};

export function createIpcMock() {
  return {
    openFileDialog: vi.fn(async () => "/path/to/meeting.mp3"),
    setMeetingSource: vi.fn(),
    transcribeMeeting: vi.fn(),
    diarizeMeeting: vi.fn(),
    createMeeting: vi.fn(),
    deleteMeeting: vi.fn(),
    listMeetings: vi.fn(),
    openMeeting: vi.fn(),
    renameMeeting: vi.fn(),
    updateSegment: vi.fn(),
    updateNotes: vi.fn(),
    generateNotes: vi.fn(),
    saveTextDialog: vi.fn(async () => null),
    listTaskModels: vi.fn(),
    downloadModel: vi.fn(),
    deleteModel: vi.fn(),
    onModelDownloadProgress: vi.fn(async () => () => {}),
    onTranscriptionPhase: vi.fn(async () => () => {}),
    onTranscriptionProgress: vi.fn(async () => () => {}),
    getSettings: vi.fn(async () => ({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    })),
    setSetting: vi.fn(),
    listStreamingSessions: vi.fn(async () => []),
    openStreamingSession: vi.fn(),
    renameStreamingSession: vi.fn(),
    deleteStreamingSession: vi.fn(),
    startStreamingSession: vi.fn(),
    stopStreamingSession: vi.fn(),
    onStreamingWindow: vi.fn(async () => () => {}),
    onStreamingSources: vi.fn(async () => () => {}),
    onStreamingSessionEnded: vi.fn(async () => () => {}),
  };
}

export async function waitForAddFileEnabled() {
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Choose file" }),
    ).not.toBeDisabled(),
  );
}

// The default flow: attach a file, then explicitly transcribe it.
export async function chooseAndTranscribe(user: {
  click: (el: Element) => Promise<void>;
}) {
  await user.click(screen.getByRole("button", { name: "Choose file" }));
  const transcribe = await screen.findByRole("button", { name: "Transcribe" });
  await waitFor(() => expect(transcribe).not.toBeDisabled());
  await user.click(transcribe);
}

// listTaskModels has no factory default (every test sets its own
// resolved/rejected value); reset it so a persistent implementation or a
// leftover "once" queue from one test can never leak into the next.
export function resetAppMocks() {
  vi.clearAllMocks();
  vi.mocked(ipc.listTaskModels).mockReset();
  vi.mocked(ipc.openFileDialog).mockResolvedValue("/path/to/meeting.mp3");
  vi.mocked(ipc.createMeeting).mockResolvedValue({ ...EMPTY_MEETING });
  vi.mocked(ipc.setMeetingSource).mockImplementation(async (_id, path) =>
    path === null ? { ...EMPTY_MEETING } : { ...ATTACHED_MEETING },
  );
  vi.mocked(ipc.transcribeMeeting).mockResolvedValue(
    transcribeResult(transcribedMeeting([HELLO_SEGMENT])),
  );
  vi.mocked(ipc.saveTextDialog).mockResolvedValue(null);
  vi.mocked(ipc.listMeetings).mockResolvedValue([]);
  vi.mocked(ipc.deleteMeeting).mockReset();
  vi.mocked(ipc.openMeeting).mockReset();
  vi.mocked(ipc.renameMeeting).mockReset();
  vi.mocked(ipc.updateSegment).mockReset();
  vi.mocked(ipc.updateNotes).mockReset();
  vi.mocked(ipc.updateSegment).mockResolvedValue(
    transcribedMeeting([HELLO_SEGMENT]),
  );
  vi.mocked(ipc.updateNotes).mockImplementation(async (notes) => ({
    ...transcribedMeeting([HELLO_SEGMENT]),
    notes,
  }));
  vi.mocked(ipc.generateNotes).mockReset();
  vi.mocked(ipc.diarizeMeeting).mockReset();
  vi.mocked(ipc.getSettings).mockResolvedValue({
    theme: "system",
    ui_language: "en",
    active_model_diarization: "none",
    export_file_type: "plain_text",
  });
  delete document.documentElement.dataset.theme;
}
