import { mockCreateIpc } from "./test/ipcMock";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import * as ipc from "./ipc";
import {
  TRANSCRIPTION_DOWNLOADED,
  TRANSCRIPTION_NOT_DOWNLOADED,
  waitForAddFileEnabled,
  chooseAndTranscribe,
  resetAppMocks,
} from "./test/appTestHarness";

vi.mock("./ipc", () => mockCreateIpc());

beforeEach(resetAppMocks);

describe("App — Settings entry point", () => {
  it("opens Settings via the header gear with AI models selected by default", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole("button", { name: "Settings" }));

    expect(screen.getByRole("tab", { name: "AI models" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("closing Settings returns to the workspace with existing transcript state intact", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);
    await waitForAddFileEnabled();

    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hello");

    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: "Close settings" }));

    expect(screen.getByDisplayValue("Hello")).toBeInTheDocument();
  });

  it("keeps the gear in the header when other header content changes", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);
    await waitForAddFileEnabled();

    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hello");

    expect(
      screen.getByRole("button", { name: "Settings" }),
    ).toBeInTheDocument();
  });
});

describe("App — transcription model availability", () => {
  it("disables Add-file with an explanation when the transcription model is not downloaded", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([
      TRANSCRIPTION_NOT_DOWNLOADED,
    ]);
    render(<App />);

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Choose file" }),
      ).toBeDisabled(),
    );
    expect(screen.getByText(/whisper/i)).toBeInTheDocument();
  });

  it("enables Add-file when the transcription model is downloaded", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    render(<App />);

    await waitForAddFileEnabled();
  });

  it("fails safe (disabled, with the warning) when the availability check itself fails", async () => {
    vi.mocked(ipc.listTaskModels).mockRejectedValue(
      new Error("IPC unavailable"),
    );
    render(<App />);

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Choose file" }),
      ).toBeDisabled(),
    );
    expect(screen.getByText(/whisper/i)).toBeInTheDocument();
  });

  it("re-enables Add-file after closing Settings once the model has been downloaded", async () => {
    // Three calls happen, in order: App's own mount check, AiModelsSection's
    // mount check when Settings opens (it independently calls
    // listTaskModels too), then App's refresh when Settings closes.
    vi.mocked(ipc.listTaskModels)
      .mockResolvedValueOnce([TRANSCRIPTION_NOT_DOWNLOADED])
      .mockResolvedValueOnce([TRANSCRIPTION_NOT_DOWNLOADED])
      .mockResolvedValueOnce([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Choose file" }),
      ).toBeDisabled(),
    );

    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: "Close settings" }));

    await waitForAddFileEnabled();
  });
});

describe("App — theme application", () => {
  it("applies the persisted theme on mount, before the user ever opens Settings", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "dark",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });

    render(<App />);

    await waitFor(() =>
      expect(document.documentElement.dataset.theme).toBe("dark"),
    );
  });
});
