import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

import {
  createRecorderDraft,
  setStreamingTranslationTargetLanguage,
  startRecorderSession,
} from "./ipc";

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
});

describe("Streaming translation IPC", () => {
  beforeEach(() => invokeMock.mockReset());

  it("persists a target language against the owning Streaming session", async () => {
    invokeMock.mockResolvedValueOnce(undefined);

    await setStreamingTranslationTargetLanguage(7, "en");

    expect(invokeMock).toHaveBeenCalledWith(
      "set_streaming_translation_target_language",
      { sessionId: 7, targetLanguage: "en" },
    );
  });
});
