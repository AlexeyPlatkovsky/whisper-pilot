import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RecorderSettingsSection } from "./RecorderSettingsSection";
import * as ipc from "./ipc";

vi.mock("./ipc", () => ({
  getSettings: vi.fn(),
  getRecorderShortcutStatus: vi.fn(),
  setRecorderShortcut: vi.fn(),
  setBubbleAlwaysOnTop: vi.fn(),
  setSetting: vi.fn(),
}));

describe("Recorder shortcut settings", () => {
  beforeEach(() => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "",
      export_file_type: "txt",
      recorder_shortcut: "Control+Option+Space",
      recorder_language: "auto",
    });
    vi.mocked(ipc.getRecorderShortcutStatus).mockResolvedValue({
      configured: "Control+Option+Space",
      active: false,
      error: "Recorder shortcut conflict",
    });
    vi.mocked(ipc.setRecorderShortcut).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "",
      export_file_type: "txt",
      recorder_shortcut: "Control+Option+Space",
      recorder_language: "auto",
    });
  });

  it("shows launch registration failure and lets the configured chord be retried", async () => {
    const user = userEvent.setup();
    render(<RecorderSettingsSection />);

    expect(
      await screen.findByText(/Shortcut status: Disabled/),
    ).toBeInTheDocument();
    expect(screen.getByText("Recorder shortcut conflict")).toBeInTheDocument();
    const retry = screen.getByRole("button", { name: "Retry shortcut" });
    expect(retry).toBeEnabled();

    vi.mocked(ipc.getRecorderShortcutStatus).mockResolvedValue({
      configured: "Control+Option+Space",
      active: true,
    });
    await user.click(retry);
    expect(ipc.setRecorderShortcut).toHaveBeenCalledWith(
      "Control+Option+Space",
    );
    await waitFor(() =>
      expect(screen.getByText(/Shortcut status: Active/)).toBeInTheDocument(),
    );
  });

  it("persists an explicit Recorder language for Qwen3-ASR", async () => {
    vi.mocked(ipc.setSetting).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
      recorder_language: "ru",
    });
    const user = userEvent.setup();
    render(<RecorderSettingsSection />);

    const language = await screen.findByRole("combobox", {
      name: "Recorder speech language",
    });
    await user.selectOptions(language, "ru");

    expect(ipc.setSetting).toHaveBeenCalledWith("recorder_language", "ru");
    expect(language).toHaveValue("ru");
  });
});
