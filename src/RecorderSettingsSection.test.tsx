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
}));

describe("Recorder shortcut settings", () => {
  beforeEach(() => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "",
      export_file_type: "txt",
      recorder_shortcut: "Control+Option+Space",
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
    });
    vi.mocked(ipc.setBubbleAlwaysOnTop).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "",
      export_file_type: "txt",
      recorder_shortcut: "Control+Option+Space",
      bubble_always_on_top: true,
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

  it("reports shortcut status failures", async () => {
    vi.mocked(ipc.getRecorderShortcutStatus).mockRejectedValueOnce(
      new Error("Shortcut service unavailable"),
    );

    render(<RecorderSettingsSection />);

    expect(
      await screen.findByText("Error: Shortcut service unavailable"),
    ).toBeInTheDocument();
    expect(screen.getByText(/Shortcut status: Disabled/)).toBeInTheDocument();
  });

  it("restores the saved shortcut when saving fails", async () => {
    vi.mocked(ipc.getRecorderShortcutStatus).mockResolvedValue({
      configured: "Control+Option+Space",
      active: true,
    });
    vi.mocked(ipc.setRecorderShortcut).mockRejectedValueOnce(
      new Error("Shortcut conflict"),
    );
    const user = userEvent.setup();
    render(<RecorderSettingsSection />);

    const input = await screen.findByRole("textbox", {
      name: "Recorder global shortcut",
    });
    await user.clear(input);
    await user.type(input, "Command+Shift+R");
    await user.click(screen.getByRole("button", { name: "Save shortcut" }));

    expect(
      await screen.findByText("Error: Shortcut conflict"),
    ).toBeInTheDocument();
    expect(input).toHaveValue("Control+Option+Space");
  });

  it("persists the floating bubble Over All setting", async () => {
    const user = userEvent.setup();
    render(<RecorderSettingsSection />);

    const toggle = await screen.findByRole("checkbox", {
      name: "Keep Recorder bubble over all windows",
    });
    await user.click(toggle);

    expect(ipc.setBubbleAlwaysOnTop).toHaveBeenCalledWith(true);
    expect(toggle).toBeChecked();
  });

  it("rolls back the floating bubble setting when persistence fails", async () => {
    vi.mocked(ipc.setBubbleAlwaysOnTop).mockRejectedValueOnce(
      new Error("Window state unavailable"),
    );
    const user = userEvent.setup();
    render(<RecorderSettingsSection />);

    const toggle = await screen.findByRole("checkbox", {
      name: "Keep Recorder bubble over all windows",
    });
    await user.click(toggle);

    expect(
      await screen.findByText("Error: Window state unavailable"),
    ).toBeInTheDocument();
    expect(toggle).not.toBeChecked();
  });
});
