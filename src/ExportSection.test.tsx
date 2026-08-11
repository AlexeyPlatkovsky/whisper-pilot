import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ExportSection } from "./ExportSection";
import * as ipc from "./ipc";

vi.mock("./ipc", () => ({
  getSettings: vi.fn(),
  setSetting: vi.fn(),
}));

describe("ExportSection", () => {
  it("shows an error when the initial load fails", async () => {
    vi.mocked(ipc.getSettings).mockRejectedValue(new Error("IPC unavailable"));

    render(<ExportSection />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      /IPC unavailable/i,
    );
  });

  it("selects the currently persisted file type (Plain text) by default", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });

    render(<ExportSection />);

    expect(
      await screen.findByRole("radio", { name: /Plain text/ }),
    ).toBeChecked();
    expect(screen.getByRole("radio", { name: /Markdown/ })).not.toBeChecked();
  });

  it("selects Markdown by default when that is the persisted file type", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "markdown",
    });

    render(<ExportSection />);

    expect(
      await screen.findByRole("radio", { name: /Markdown/ }),
    ).toBeChecked();
  });

  it("clicking Markdown persists it", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    vi.mocked(ipc.setSetting).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "markdown",
    });
    const user = userEvent.setup();

    render(<ExportSection />);
    await user.click(await screen.findByRole("radio", { name: /Markdown/ }));

    expect(ipc.setSetting).toHaveBeenCalledWith("export_file_type", "markdown");
    expect(screen.getByRole("radio", { name: /Markdown/ })).toBeChecked();
  });

  it("reverts to the previous selection and shows an error when persisting fails", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    vi.mocked(ipc.setSetting).mockRejectedValue(new Error("disk full"));
    const user = userEvent.setup();

    render(<ExportSection />);
    await user.click(await screen.findByRole("radio", { name: /Markdown/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent(/disk full/i);
    expect(screen.getByRole("radio", { name: /Plain text/ })).toBeChecked();
  });
});
