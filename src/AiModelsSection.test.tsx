import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AiModelsSection } from "./AiModelsSection";
import * as ipc from "./ipc";

vi.mock("./ipc", () => ({
  listTaskModels: vi.fn(),
  downloadModel: vi.fn(),
  deleteModel: vi.fn(),
  onModelDownloadProgress: vi.fn(async () => () => {}),
  getSettings: vi.fn(),
  setSetting: vi.fn(),
}));

const TRANSCRIPTION_NOT_DOWNLOADED = {
  id: "transcription",
  task: "transcription",
  label: "Whisper large-v3-turbo (Q8)",
  downloaded: false,
  size_bytes: 874_188_075,
  recommended: false,
};

const TRANSCRIPTION_DOWNLOADED = {
  ...TRANSCRIPTION_NOT_DOWNLOADED,
  downloaded: true,
};

const CAMPPLUS_DOWNLOADED = {
  id: "diarization-campplus",
  task: "diarization",
  label: "CAM++",
  downloaded: true,
  size_bytes: 36_555_422,
  recommended: false,
};

const TITANET_NOT_DOWNLOADED = {
  id: "diarization-titanet-large",
  task: "diarization",
  label: "TitaNet-large",
  downloaded: false,
  size_bytes: 108_363_937,
  recommended: true,
};

/** Hand back the progress callback the component registered on mount. */
function captureProgressHandler(): (p: ipc.ModelDownloadProgress) => void {
  let handler: (p: ipc.ModelDownloadProgress) => void = () => {};
  vi.mocked(ipc.onModelDownloadProgress).mockImplementation(async (h) => {
    handler = h;
    return () => {};
  });
  return (p) => handler(p);
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(ipc.onModelDownloadProgress).mockResolvedValue(() => {});
  vi.mocked(ipc.getSettings).mockResolvedValue({
    theme: "system",
    ui_language: "en",
    active_model_diarization: "none",
  });
});

describe("AiModelsSection", () => {
  it("shows a Download button for a not-downloaded model", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([
      TRANSCRIPTION_NOT_DOWNLOADED,
    ]);

    render(<AiModelsSection />);

    expect(
      await screen.findByRole("button", {
        name: /download whisper large-v3-turbo/i,
      }),
    ).toBeInTheDocument();
  });

  it("shows Ready and a Delete button for an already-downloaded model, no Download button", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);

    render(<AiModelsSection />);

    expect(await screen.findByText("Ready")).toBeInTheDocument();
    expect(
      await screen.findByRole("button", {
        name: /delete whisper large-v3-turbo/i,
      }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /download whisper/i }),
    ).not.toBeInTheDocument();
  });

  it("clicking Delete opens a confirmation dialog before removing the model", async () => {
    vi.mocked(ipc.listTaskModels)
      .mockResolvedValueOnce([TRANSCRIPTION_DOWNLOADED])
      .mockResolvedValueOnce([TRANSCRIPTION_NOT_DOWNLOADED]);
    vi.mocked(ipc.deleteModel).mockResolvedValue(undefined);
    const user = userEvent.setup();

    render(<AiModelsSection />);
    await user.click(
      await screen.findByRole("button", {
        name: /delete whisper large-v3-turbo/i,
      }),
    );

    expect(ipc.deleteModel).not.toHaveBeenCalled();
    await user.click(await screen.findByRole("button", { name: "Delete" }));

    expect(ipc.deleteModel).toHaveBeenCalledWith("transcription");
    expect(
      await screen.findByRole("button", {
        name: /download whisper large-v3-turbo/i,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /delete whisper/i }),
    ).toBeDisabled();
  });

  it("clicking Cancel in the confirmation dialog leaves the model untouched", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();

    render(<AiModelsSection />);
    await user.click(
      await screen.findByRole("button", {
        name: /delete whisper large-v3-turbo/i,
      }),
    );
    await user.click(await screen.findByRole("button", { name: "Cancel" }));

    expect(ipc.deleteModel).not.toHaveBeenCalled();
    expect(
      await screen.findByRole("button", {
        name: /delete whisper large-v3-turbo/i,
      }),
    ).toBeInTheDocument();
  });

  it("shows an error message when delete fails", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.deleteModel).mockRejectedValue(
      new Error("permission denied"),
    );
    const user = userEvent.setup();

    render(<AiModelsSection />);
    await user.click(
      await screen.findByRole("button", {
        name: /delete whisper large-v3-turbo/i,
      }),
    );
    await user.click(await screen.findByRole("button", { name: "Delete" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      /permission denied/i,
    );
  });

  it("shows a progress bar while downloading and Ready after success", async () => {
    vi.mocked(ipc.listTaskModels)
      .mockResolvedValueOnce([TRANSCRIPTION_NOT_DOWNLOADED])
      .mockResolvedValueOnce([TRANSCRIPTION_DOWNLOADED]);
    const progressHandler = captureProgressHandler();
    let resolveDownload: () => void = () => {};
    vi.mocked(ipc.downloadModel).mockReturnValue(
      new Promise<void>((resolve) => {
        resolveDownload = resolve;
      }),
    );
    const user = userEvent.setup();

    render(<AiModelsSection />);
    await user.click(
      await screen.findByRole("button", {
        name: /download whisper large-v3-turbo/i,
      }),
    );

    progressHandler({
      id: "transcription",
      fraction: 0.5,
      stage: "downloading",
    });
    await waitFor(() =>
      expect(screen.getByRole("progressbar")).toHaveAttribute("value", "0.5"),
    );

    resolveDownload();
    expect(await screen.findByText("Ready")).toBeInTheDocument();
  });

  // The bytes landing is not the end of the job: the backend still hashes the
  // whole file before the model counts as ready. Showing "Downloading" at a
  // full bar for that stretch reads as a hung download.
  it("shows Verifying instead of Downloading once the transfer moves to hash verification", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([
      TRANSCRIPTION_NOT_DOWNLOADED,
    ]);
    const progressHandler = captureProgressHandler();
    vi.mocked(ipc.downloadModel).mockReturnValue(new Promise(() => {}));
    const user = userEvent.setup();

    render(<AiModelsSection />);
    await user.click(
      await screen.findByRole("button", {
        name: /download whisper large-v3-turbo/i,
      }),
    );
    progressHandler({ id: "transcription", fraction: 1, stage: "verifying" });

    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("Verifying...")).toBeInTheDocument();
    expect(
      within(dialog).queryByText("Downloading..."),
    ).not.toBeInTheDocument();
  });

  // Closing the modal only dismisses the modal — the download keeps running,
  // so the row's spinner has to say what it is still waiting for.
  it("reports download progress and verification in the model row after the modal is closed", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([
      TRANSCRIPTION_NOT_DOWNLOADED,
    ]);
    const progressHandler = captureProgressHandler();
    vi.mocked(ipc.downloadModel).mockReturnValue(new Promise(() => {}));
    const user = userEvent.setup();

    render(<AiModelsSection />);
    await user.click(
      await screen.findByRole("button", {
        name: /download whisper large-v3-turbo/i,
      }),
    );
    progressHandler({
      id: "transcription",
      fraction: 0.42,
      stage: "downloading",
    });
    await user.click(screen.getByRole("button", { name: "Close" }));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(await screen.findByText("42%")).toBeInTheDocument();

    progressHandler({ id: "transcription", fraction: 1, stage: "verifying" });

    expect(await screen.findByText("Verifying...")).toBeInTheDocument();
  });

  it("shows an error message when the download fails", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([
      TRANSCRIPTION_NOT_DOWNLOADED,
    ]);
    vi.mocked(ipc.downloadModel).mockRejectedValue(
      new Error("model download failed: connection reset"),
    );
    const user = userEvent.setup();

    render(<AiModelsSection />);
    await user.click(
      await screen.findByRole("button", {
        name: /download whisper large-v3-turbo/i,
      }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(
      /connection reset/i,
    );
  });

  it("shows a load error when listTaskModels rejects", async () => {
    vi.mocked(ipc.listTaskModels).mockRejectedValue(
      new Error("models unavailable"),
    );

    render(<AiModelsSection />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      /models unavailable/i,
    );
  });

  it("closes the download modal when the Close button is clicked", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([
      TRANSCRIPTION_NOT_DOWNLOADED,
    ]);
    vi.mocked(ipc.downloadModel).mockReturnValue(new Promise(() => {}));
    const user = userEvent.setup();

    render(<AiModelsSection />);
    await user.click(
      await screen.findByRole("button", {
        name: /download whisper large-v3-turbo/i,
      }),
    );

    expect(await screen.findByRole("dialog")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Close" }));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  // Both sections have to use the same native control, or "selected" renders
  // as two visibly different things in one Settings screen.
  describe("transcription model selection", () => {
    it("marks the downloaded transcription model with a checked radio", async () => {
      vi.mocked(ipc.listTaskModels).mockResolvedValue([
        TRANSCRIPTION_DOWNLOADED,
      ]);

      render(<AiModelsSection />);

      expect(
        await screen.findByRole("radio", {
          name: "Whisper large-v3-turbo (Q8)",
        }),
      ).toBeChecked();
    });

    it("leaves the transcription radio unchecked and disabled until it is downloaded", async () => {
      vi.mocked(ipc.listTaskModels).mockResolvedValue([
        TRANSCRIPTION_NOT_DOWNLOADED,
      ]);

      render(<AiModelsSection />);

      const radio = await screen.findByRole("radio", {
        name: "Whisper large-v3-turbo (Q8)",
      });
      expect(radio).not.toBeChecked();
      expect(radio).toBeDisabled();
    });
  });

  describe("diarization model selection", () => {
    it("renders None plus one radio per downloaded-state variant, disabling the undownloaded one", async () => {
      vi.mocked(ipc.listTaskModels).mockResolvedValue([
        CAMPPLUS_DOWNLOADED,
        TITANET_NOT_DOWNLOADED,
      ]);

      render(<AiModelsSection />);

      const none = await screen.findByRole("radio", { name: "None" });
      const campplus = screen.getByRole("radio", { name: "CAM++" });
      const titanet = screen.getByRole("radio", { name: "TitaNet-large" });
      expect(none).toBeEnabled();
      expect(campplus).toBeEnabled();
      expect(titanet).toBeDisabled();
    });

    it("shows a Recommended badge on the recommended variant", async () => {
      vi.mocked(ipc.listTaskModels).mockResolvedValue([
        CAMPPLUS_DOWNLOADED,
        TITANET_NOT_DOWNLOADED,
      ]);

      render(<AiModelsSection />);

      expect(await screen.findByText("Recommended")).toBeInTheDocument();
    });

    it("selects None by default when active_model_diarization is none", async () => {
      vi.mocked(ipc.listTaskModels).mockResolvedValue([CAMPPLUS_DOWNLOADED]);

      render(<AiModelsSection />);

      expect(await screen.findByRole("radio", { name: "None" })).toBeChecked();
      expect(screen.getByRole("radio", { name: "CAM++" })).not.toBeChecked();
    });

    it("selecting a downloaded variant persists it via setSetting", async () => {
      vi.mocked(ipc.listTaskModels).mockResolvedValue([CAMPPLUS_DOWNLOADED]);
      vi.mocked(ipc.setSetting).mockResolvedValue({
        theme: "system",
        ui_language: "en",
        active_model_diarization: "campplus",
      });
      const user = userEvent.setup();

      render(<AiModelsSection />);
      await user.click(await screen.findByRole("radio", { name: "CAM++" }));

      expect(ipc.setSetting).toHaveBeenCalledWith(
        "active_model.diarization",
        "campplus",
      );
      expect(screen.getByRole("radio", { name: "CAM++" })).toBeChecked();
    });

    // Turning diarization off again is the user's escape hatch when a model
    // misbehaves on their audio, so it has to persist like any other choice.
    it("selecting None turns diarization off and persists it", async () => {
      vi.mocked(ipc.listTaskModels).mockResolvedValue([CAMPPLUS_DOWNLOADED]);
      vi.mocked(ipc.getSettings).mockResolvedValue({
        theme: "system",
        ui_language: "en",
        active_model_diarization: "campplus",
      });
      vi.mocked(ipc.setSetting).mockResolvedValue({
        theme: "system",
        ui_language: "en",
        active_model_diarization: "none",
      });
      const user = userEvent.setup();

      render(<AiModelsSection />);
      await waitFor(() =>
        expect(screen.getByRole("radio", { name: "CAM++" })).toBeChecked(),
      );
      await user.click(screen.getByRole("radio", { name: "None" }));

      expect(ipc.setSetting).toHaveBeenCalledWith(
        "active_model.diarization",
        "none",
      );
      expect(screen.getByRole("radio", { name: "None" })).toBeChecked();
    });

    it("reverts the selection and shows an error when persisting fails", async () => {
      vi.mocked(ipc.listTaskModels).mockResolvedValue([CAMPPLUS_DOWNLOADED]);
      vi.mocked(ipc.setSetting).mockRejectedValue(
        new Error("unknown diarization model id"),
      );
      const user = userEvent.setup();

      render(<AiModelsSection />);
      await user.click(await screen.findByRole("radio", { name: "CAM++" }));

      expect(await screen.findByRole("alert")).toHaveTextContent(
        /unknown diarization model id/i,
      );
      expect(await screen.findByRole("radio", { name: "None" })).toBeChecked();
    });

    it("reverts the selection to None after deleting the currently active variant", async () => {
      vi.mocked(ipc.getSettings)
        .mockResolvedValueOnce({
          theme: "system",
          ui_language: "en",
          active_model_diarization: "campplus",
        })
        .mockResolvedValueOnce({
          theme: "system",
          ui_language: "en",
          active_model_diarization: "none",
        });
      vi.mocked(ipc.listTaskModels)
        .mockResolvedValueOnce([CAMPPLUS_DOWNLOADED, TITANET_NOT_DOWNLOADED])
        .mockResolvedValueOnce([
          { ...CAMPPLUS_DOWNLOADED, downloaded: false },
          TITANET_NOT_DOWNLOADED,
        ]);
      vi.mocked(ipc.deleteModel).mockResolvedValue(undefined);
      const user = userEvent.setup();

      render(<AiModelsSection />);
      expect(await screen.findByRole("radio", { name: "CAM++" })).toBeChecked();

      await user.click(screen.getByRole("button", { name: "Delete CAM++" }));
      await user.click(await screen.findByRole("button", { name: "Delete" }));

      expect(ipc.deleteModel).toHaveBeenCalledWith("diarization-campplus");
      expect(await screen.findByRole("radio", { name: "None" })).toBeChecked();
    });
  });
});
