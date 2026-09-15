import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as ipc from "./ipc";
import { useModelLibrary } from "./useModelLibrary";

vi.mock("./ipc", () => ({
  deleteModel: vi.fn(),
  downloadModel: vi.fn(),
  getSettings: vi.fn(),
  listTaskModels: vi.fn(),
  onModelDownloadProgress: vi.fn(),
  setSetting: vi.fn(),
}));

const LLM_MODEL: ipc.TaskModel = {
  id: "qwen-llm",
  task: "llm",
  label: "Qwen LLM",
  downloaded: false,
  size_bytes: 1_000,
  recommended: true,
};

describe("useModelLibrary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(ipc.listTaskModels).mockResolvedValue([]);
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      active_model_llm: "current-llm",
      active_model_transcription: "current-asr",
      export_file_type: "plain_text",
    });
    vi.mocked(ipc.onModelDownloadProgress).mockResolvedValue(() => {});
    vi.mocked(ipc.downloadModel).mockResolvedValue(undefined);
    vi.mocked(ipc.setSetting).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
  });

  it("restores LLM and transcription selections when persistence fails", async () => {
    vi.mocked(ipc.setSetting).mockRejectedValue(new Error("settings locked"));
    const { result } = renderHook(() => useModelLibrary());

    await waitFor(() => expect(result.current.llmModel).toBe("current-llm"));
    await act(async () => {
      await result.current.handleSelectLlmModel("next-llm");
      await result.current.handleSelectTranscriptionModel("next-asr");
    });

    expect(result.current.llmModel).toBe("current-llm");
    expect(result.current.llmSelectError).toBe("Error: settings locked");
    expect(result.current.transcriptionModel).toBe("current-asr");
    expect(result.current.transcriptionSelectError).toBe(
      "Error: settings locked",
    );
  });

  it("automatically selects the first downloaded LLM when none is active", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    vi.mocked(ipc.listTaskModels)
      .mockResolvedValueOnce([LLM_MODEL])
      .mockResolvedValueOnce([{ ...LLM_MODEL, downloaded: true }]);
    const { result } = renderHook(() => useModelLibrary());

    await waitFor(() => expect(result.current.models).toEqual([LLM_MODEL]));
    await act(async () => {
      await result.current.handleDownload(LLM_MODEL.id);
    });

    expect(ipc.setSetting).toHaveBeenCalledWith(
      "active_model.llm",
      LLM_MODEL.id,
    );
    expect(result.current.llmModel).toBe(LLM_MODEL.id);
  });

  it("tracks elapsed time while a model download remains active", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([LLM_MODEL]);
    vi.mocked(ipc.downloadModel).mockReturnValue(new Promise(() => {}));
    const { result, unmount } = renderHook(() => useModelLibrary());
    await waitFor(() => expect(result.current.models).toEqual([LLM_MODEL]));

    vi.useFakeTimers();
    try {
      act(() => {
        void result.current.handleDownload(LLM_MODEL.id);
      });
      expect(result.current.downloadModalId).toBe(LLM_MODEL.id);

      act(() => {
        vi.advanceTimersByTime(2_000);
      });
      expect(result.current.elapsed).toBe(2);
    } finally {
      unmount();
      vi.useRealTimers();
    }
  });
});
