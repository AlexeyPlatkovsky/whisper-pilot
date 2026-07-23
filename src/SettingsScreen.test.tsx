import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SettingsScreen } from "./SettingsScreen";

// AiModelsSection and AppearanceSection (section content) talk to the
// Tauri IPC layer; mock it at the boundary so these shell/nav-level tests
// don't depend on their internals (covered by their own test files).
vi.mock("./ipc", () => ({
  listTaskModels: vi.fn(async () => []),
  downloadModel: vi.fn(),
  deleteModel: vi.fn(),
  onModelDownloadProgress: vi.fn(async () => () => {}),
  getSettings: vi.fn(async () => ({ theme: "system", ui_language: "en" })),
  setSetting: vi.fn(),
}));

describe("SettingsScreen", () => {
  it("selects the AI models section by default", () => {
    render(<SettingsScreen onClose={vi.fn()} />);

    expect(screen.getByRole("tab", { name: "AI models" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("switches the visible section when a nav item is clicked", async () => {
    const user = userEvent.setup();
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Appearance" }));

    expect(screen.getByRole("tab", { name: "Appearance" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("tab", { name: "AI models" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    expect(
      await screen.findByRole("group", { name: "Theme" }),
    ).toBeInTheDocument();
  });

  it("calls onClose when the close button is clicked", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<SettingsScreen onClose={onClose} />);

    await user.click(screen.getByRole("button", { name: "Close settings" }));

    expect(onClose).toHaveBeenCalledOnce();
  });

  it("calls onClose when Escape is pressed", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<SettingsScreen onClose={onClose} />);

    await user.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalledOnce();
  });

  it("reaches the close button and every section tab via Tab alone", async () => {
    const user = userEvent.setup();
    render(<SettingsScreen onClose={vi.fn()} />);

    const closeButton = screen.getByRole("button", { name: "Close settings" });
    const tabs = screen.getAllByRole("tab");

    await user.tab();
    expect(document.activeElement).toBe(closeButton);
    for (const tab of tabs) {
      await user.tab();
      expect(document.activeElement).toBe(tab);
    }
  });

  it("switches to the App language section", async () => {
    const user = userEvent.setup();
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "App language" }));

    expect(screen.getByRole("tab", { name: "App language" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("radio", { name: "English" })).toBeInTheDocument();
  });
});
