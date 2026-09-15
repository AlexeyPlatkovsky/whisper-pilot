import { beforeEach, describe, expect, it, vi } from "vitest";
import * as ipcModule from "./ipc";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: listenMock,
}));

import {
  clearMeeting,
  clearRecorderRecording,
  clearStreamingSession,
  createRecorderDraft,
  previewStreamingTranslation,
  setStreamingTranslationTargetLanguage,
  startRecorderSession,
} from "./ipc";

describe("Clear IPC", () => {
  beforeEach(() => invokeMock.mockReset());

  it("clears only the derived Meeting content", async () => {
    invokeMock.mockResolvedValueOnce({ id: 3, segments: [], mfu: null });

    await clearMeeting(3);

    expect(invokeMock).toHaveBeenCalledWith("clear_meeting", { id: 3 });
  });

  it("clears only the derived Streaming content", async () => {
    invokeMock.mockResolvedValueOnce({ id: 5, windows: [], mfu: null });

    await clearStreamingSession(5);

    expect(invokeMock).toHaveBeenCalledWith("clear_streaming_session", {
      id: 5,
    });
  });
});

describe("Recorder IPC", () => {
  beforeEach(() => invokeMock.mockReset());

  it("creates an audio-free draft through the dedicated command", async () => {
    invokeMock.mockResolvedValueOnce({ id: 7, is_draft: true });

    await expect(createRecorderDraft()).resolves.toMatchObject({
      id: 7,
      is_draft: true,
    });
    expect(invokeMock).toHaveBeenCalledWith("create_recorder_draft");
  });

  it("passes the selected draft id when starting Recorder", async () => {
    invokeMock.mockResolvedValueOnce({ id: 7, is_draft: false });

    await startRecorderSession(7);

    expect(invokeMock).toHaveBeenCalledWith("start_recorder_session", {
      draftId: 7,
    });
  });

  it("clears the Recorder audio and transcript through the recording command", async () => {
    invokeMock.mockResolvedValueOnce({ id: 7, is_draft: true, segments: [] });

    await clearRecorderRecording(7);

    expect(invokeMock).toHaveBeenCalledWith("clear_recorder_recording", {
      id: 7,
    });
  });
});

describe("Streaming translation IPC", () => {
  beforeEach(() => invokeMock.mockReset());

  it("requests an ephemeral provisional translation without a persistence key", async () => {
    invokeMock.mockResolvedValueOnce("Draft translation");

    await expect(
      previewStreamingTranslation("ru", "Current partial", "Prior context"),
    ).resolves.toBe("Draft translation");
    expect(invokeMock).toHaveBeenCalledWith("preview_streaming_translation", {
      targetLanguage: "ru",
      text: "Current partial",
      context: "Prior context",
    });
  });

  it("persists a target language against the owning Streaming session", async () => {
    invokeMock.mockResolvedValueOnce(undefined);

    await setStreamingTranslationTargetLanguage(7, "en");

    expect(invokeMock).toHaveBeenCalledWith(
      "set_streaming_translation_target_language",
      { sessionId: 7, targetLanguage: "en" },
    );
  });
});

describe("IPC command contract", () => {
  beforeEach(() => invokeMock.mockReset().mockResolvedValue(undefined));

  const commandCases: Array<
    [
      string,
      () => Promise<unknown>,
      string,
      Record<string, unknown> | undefined,
    ]
  > = [
    [
      "createMeeting",
      () => ipcModule.createMeeting(),
      "create_meeting",
      undefined,
    ],
    [
      "listMeetings",
      () => ipcModule.listMeetings(),
      "list_meetings",
      undefined,
    ],
    ["openMeeting", () => ipcModule.openMeeting(3), "open_meeting", { id: 3 }],
    [
      "renameMeeting",
      () => ipcModule.renameMeeting(3, "Renamed"),
      "rename_meeting",
      { id: 3, title: "Renamed" },
    ],
    [
      "deleteMeeting",
      () => ipcModule.deleteMeeting(3),
      "delete_meeting",
      { id: 3 },
    ],
    [
      "updateSegment",
      () => ipcModule.updateSegment(3, 2, "Edited"),
      "update_segment",
      { id: 3, index: 2, text: "Edited" },
    ],
    [
      "updateMfu",
      () =>
        ipcModule.updateMfu({
          meeting_id: 3,
          summary: "S",
          decisions: "D",
          action_items: "A",
          open_questions: "Q",
          participants: "P",
        }),
      "update_mfu",
      {
        mfu: {
          meeting_id: 3,
          summary: "S",
          decisions: "D",
          action_items: "A",
          open_questions: "Q",
          participants: "P",
        },
      },
    ],
    [
      "openFileDialog",
      () => ipcModule.openFileDialog(),
      "open_file_dialog",
      undefined,
    ],
    [
      "setMeetingSource",
      () => ipcModule.setMeetingSource(3, "/tmp/source.wav"),
      "set_meeting_source",
      { id: 3, path: "/tmp/source.wav" },
    ],
    [
      "transcribeMeeting",
      () => ipcModule.transcribeMeeting(3),
      "transcribe_meeting",
      { id: 3 },
    ],
    [
      "diarizeMeeting",
      () => ipcModule.diarizeMeeting(3),
      "diarize_meeting",
      { id: 3 },
    ],
    ["generateMfu", () => ipcModule.generateMfu(3), "generate_mfu", { id: 3 }],
    [
      "saveTextDialog",
      () => ipcModule.saveTextDialog("Body", "note.txt"),
      "save_text_dialog",
      { content: "Body", defaultName: "note.txt" },
    ],
    ["getSettings", () => ipcModule.getSettings(), "get_settings", undefined],
    [
      "setSetting",
      () => ipcModule.setSetting("theme", "dark"),
      "set_setting",
      { key: "theme", value: "dark" },
    ],
    [
      "collapseToBubble",
      () => ipcModule.collapseToBubble(),
      "collapse_to_bubble",
      undefined,
    ],
    [
      "restoreMainFromBubble",
      () => ipcModule.restoreMainFromBubble(),
      "restore_main_from_bubble",
      undefined,
    ],
    [
      "setBubbleAlwaysOnTop",
      () => ipcModule.setBubbleAlwaysOnTop(true),
      "set_bubble_always_on_top",
      { value: true },
    ],
    [
      "setRecorderShortcut",
      () => ipcModule.setRecorderShortcut("Cmd+Shift+R"),
      "set_recorder_shortcut",
      { value: "Cmd+Shift+R" },
    ],
    [
      "getRecorderShortcutStatus",
      () => ipcModule.getRecorderShortcutStatus(),
      "get_recorder_shortcut_status",
      undefined,
    ],
    [
      "getCloudProviderConfig",
      () => ipcModule.getCloudProviderConfig(),
      "get_cloud_provider_config",
      undefined,
    ],
    [
      "selectCloudProvider",
      () => ipcModule.selectCloudProvider("openai"),
      "select_cloud_provider",
      { provider: "openai" },
    ],
    [
      "verifyCloudProviderApiKey",
      () => ipcModule.verifyCloudProviderApiKey("openai", "secret"),
      "verify_cloud_provider_api_key",
      { provider: "openai", apiKey: "secret" },
    ],
    [
      "saveCloudProviderApiKey",
      () => ipcModule.saveCloudProviderApiKey("deepgram", "secret"),
      "save_cloud_provider_api_key",
      { provider: "deepgram", apiKey: "secret" },
    ],
    [
      "removeCloudProviderApiKey",
      () => ipcModule.removeCloudProviderApiKey("assemblyai"),
      "remove_cloud_provider_api_key",
      { provider: "assemblyai" },
    ],
    [
      "listTaskModels",
      () => ipcModule.listTaskModels(),
      "list_task_models",
      undefined,
    ],
    [
      "downloadModel",
      () => ipcModule.downloadModel("model"),
      "download_model",
      { id: "model" },
    ],
    [
      "deleteModel",
      () => ipcModule.deleteModel("model"),
      "delete_model",
      { id: "model" },
    ],
    [
      "generateStreamingMfu",
      () => ipcModule.generateStreamingMfu(5),
      "generate_streaming_mfu",
      { id: 5 },
    ],
    [
      "generateStreamingPrettify",
      () => ipcModule.generateStreamingPrettify(5),
      "generate_streaming_prettify",
      { id: 5 },
    ],
    [
      "acceptStreamingPrettify",
      () => ipcModule.acceptStreamingPrettify(5, "Clean"),
      "accept_streaming_prettify",
      { id: 5, text: "Clean" },
    ],
    [
      "revertStreamingPrettify",
      () => ipcModule.revertStreamingPrettify(5),
      "revert_streaming_prettify",
      { id: 5 },
    ],
    [
      "listStreamingSessions",
      () => ipcModule.listStreamingSessions(),
      "list_streaming_sessions",
      undefined,
    ],
    [
      "openStreamingSession",
      () => ipcModule.openStreamingSession(5),
      "open_streaming_session",
      { id: 5 },
    ],
    [
      "renameStreamingSession",
      () => ipcModule.renameStreamingSession(5, "Stream"),
      "rename_streaming_session",
      { id: 5, title: "Stream" },
    ],
    [
      "deleteStreamingSession",
      () => ipcModule.deleteStreamingSession(5),
      "delete_streaming_session",
      { id: 5 },
    ],
    [
      "createStreamingSession",
      () => ipcModule.createStreamingSession(),
      "create_streaming_session",
      undefined,
    ],
    [
      "startStreamingSession",
      () => ipcModule.startStreamingSession(5, "cloud"),
      "start_streaming_session",
      { sessionId: 5, engine: "cloud" },
    ],
    [
      "stopStreamingSession",
      () => ipcModule.stopStreamingSession(),
      "stop_streaming_session",
      undefined,
    ],
    [
      "getLiveCaptureSnapshot",
      () => ipcModule.getLiveCaptureSnapshot(),
      "get_live_capture_snapshot",
      undefined,
    ],
    [
      "getMicrophonePermissionStatus",
      () => ipcModule.getMicrophonePermissionStatus(),
      "get_microphone_permission_status",
      undefined,
    ],
    [
      "requestMicrophonePermission",
      () => ipcModule.requestMicrophonePermission(),
      "request_microphone_permission",
      undefined,
    ],
    [
      "showRecorderWorkspace",
      () => ipcModule.showRecorderWorkspace(),
      "show_recorder_workspace",
      undefined,
    ],
    [
      "listRecorderSessions",
      () => ipcModule.listRecorderSessions(),
      "list_recorder_sessions",
      undefined,
    ],
    [
      "openRecorderSession",
      () => ipcModule.openRecorderSession(7),
      "open_recorder_session",
      { id: 7 },
    ],
    [
      "renameRecorderSession",
      () => ipcModule.renameRecorderSession(7, "Record"),
      "rename_recorder_session",
      { id: 7, title: "Record" },
    ],
    [
      "deleteRecorderSession",
      () => ipcModule.deleteRecorderSession(7),
      "delete_recorder_session",
      { id: 7 },
    ],
    [
      "recoverRecorderSession",
      () => ipcModule.recoverRecorderSession(7),
      "recover_recorder_session",
      { id: 7 },
    ],
    [
      "stopRecorderSession",
      () => ipcModule.stopRecorderSession(),
      "stop_recorder_session",
      undefined,
    ],
    [
      "updateRecorderSegment",
      () => ipcModule.updateRecorderSegment(7, 9, "Edited"),
      "update_recorder_segment",
      { sessionId: 7, segmentId: 9, text: "Edited" },
    ],
    [
      "exportRecorderWav",
      () => ipcModule.exportRecorderWav(7),
      "export_recorder_wav",
      { id: 7 },
    ],
    [
      "generateRecorderPolish",
      () => ipcModule.generateRecorderPolish(7),
      "generate_recorder_polish",
      { id: 7 },
    ],
    [
      "acceptRecorderPolish",
      () => ipcModule.acceptRecorderPolish(7, "Clean"),
      "accept_recorder_polish",
      { id: 7, text: "Clean" },
    ],
    [
      "revertRecorderPolish",
      () => ipcModule.revertRecorderPolish(7),
      "revert_recorder_polish",
      { id: 7 },
    ],
    [
      "translateStreamingWindow",
      () => ipcModule.translateStreamingWindow(5, 2, "en", "Text", "Context"),
      "translate_streaming_window",
      {
        sessionId: 5,
        windowIndex: 2,
        targetLanguage: "en",
        text: "Text",
        context: "Context",
      },
    ],
    [
      "listStreamingTranslations",
      () => ipcModule.listStreamingTranslations(5, "ru"),
      "list_streaming_translations",
      { sessionId: 5, targetLanguage: "ru" },
    ],
    [
      "setStreamingTranslationEnabled",
      () => ipcModule.setStreamingTranslationEnabled(5, true),
      "set_streaming_translation_enabled",
      { sessionId: 5, enabled: true },
    ],
  ];

  it.each(commandCases)(
    "maps %s to its exact Tauri command",
    async (_name, call, command, args) => {
      await call();

      if (args === undefined) expect(invokeMock).toHaveBeenCalledWith(command);
      else expect(invokeMock).toHaveBeenCalledWith(command, args);
    },
  );
});

describe("IPC event contract", () => {
  beforeEach(() => {
    listenMock.mockReset().mockImplementation(async (_name, callback) => {
      callback({ payload: { marker: "payload" } });
      return vi.fn();
    });
  });

  const eventCases: Array<
    [string, (handler: (payload: unknown) => void) => Promise<unknown>, boolean]
  > = [
    [
      "model_download_progress",
      (handler) => ipcModule.onModelDownloadProgress(handler as never),
      true,
    ],
    [
      "transcription_phase",
      (handler) => ipcModule.onTranscriptionPhase(handler as never),
      true,
    ],
    [
      "transcription_progress",
      (handler) => ipcModule.onTranscriptionProgress(handler as never),
      true,
    ],
    [
      "open_recorder_workspace",
      (handler) => ipcModule.onOpenRecorderWorkspace(() => handler(undefined)),
      false,
    ],
    [
      "recorder_session_changed",
      (handler) => ipcModule.onRecorderSessionChanged(handler as never),
      true,
    ],
    [
      "recorder_segment_committed",
      (handler) => ipcModule.onRecorderSegmentCommitted(handler as never),
      true,
    ],
    [
      "recorder_partial",
      (handler) => ipcModule.onRecorderPartial(handler as never),
      true,
    ],
    [
      "recorder_error",
      (handler) => ipcModule.onRecorderError(handler as never),
      true,
    ],
    [
      "live_capture_state",
      (handler) => ipcModule.onLiveCaptureState(handler as never),
      true,
    ],
    [
      "streaming_window",
      (handler) => ipcModule.onStreamingWindow(handler as never),
      true,
    ],
    [
      "streaming_sources",
      (handler) => ipcModule.onStreamingSources(handler as never),
      true,
    ],
    [
      "streaming_session_ended",
      (handler) => ipcModule.onStreamingSessionEnded(handler as never),
      true,
    ],
    [
      "streaming_partial",
      (handler) => ipcModule.onStreamingPartial(handler as never),
      true,
    ],
    [
      "streaming_error",
      (handler) => ipcModule.onStreamingError(handler as never),
      true,
    ],
  ];

  it.each(eventCases)(
    "subscribes to %s and forwards its event",
    async (name, subscribe, forwardsPayload) => {
      const handler = vi.fn();

      await subscribe(handler);

      expect(listenMock).toHaveBeenCalledWith(name, expect.any(Function));
      if (forwardsPayload)
        expect(handler).toHaveBeenCalledWith({ marker: "payload" });
      else expect(handler).toHaveBeenCalledOnce();
    },
  );
});
