import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AppearanceSection } from "./AppearanceSection";
import * as ipc from "./ipc";
import { readCssBundle } from "./test/readCssBundle";

vi.mock("./ipc", () => ({
  getSettings: vi.fn(),
  setSetting: vi.fn(),
}));

const originalMatchMedia = window.matchMedia;

afterEach(() => {
  delete document.documentElement.dataset.theme;
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: originalMatchMedia,
  });
});

describe("AppearanceSection", () => {
  it("shows an error when the initial theme load fails", async () => {
    vi.mocked(ipc.getSettings).mockRejectedValue(new Error("IPC unavailable"));

    render(<AppearanceSection />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      /IPC unavailable/i,
    );
  });

  it("selects the currently persisted theme (System) by default", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });

    render(<AppearanceSection />);

    expect(await screen.findByRole("radio", { name: "System" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Light" })).not.toBeChecked();
    expect(screen.getByRole("radio", { name: "Dark" })).not.toBeChecked();
  });

  it("selects Dark by default when that is the persisted theme", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "dark",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });

    render(<AppearanceSection />);

    expect(await screen.findByRole("radio", { name: "Dark" })).toBeChecked();
  });

  it("clicking Dark applies it immediately and persists it", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    vi.mocked(ipc.setSetting).mockResolvedValue({
      theme: "dark",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    const user = userEvent.setup();

    render(<AppearanceSection />);
    await user.click(await screen.findByRole("radio", { name: "Dark" }));

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(ipc.setSetting).toHaveBeenCalledWith("theme", "dark");
    expect(screen.getByRole("radio", { name: "Dark" })).toBeChecked();
  });

  it("clicking System clears the data-theme attribute", async () => {
    // Simulate App.tsx having already applied "dark" on mount, so this
    // test can actually prove the attribute gets cleared, not just that
    // it was never set.
    document.documentElement.dataset.theme = "dark";
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "dark",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    vi.mocked(ipc.setSetting).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    const user = userEvent.setup();

    render(<AppearanceSection />);
    await user.click(await screen.findByRole("radio", { name: "System" }));

    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it("switches from a dark System preference to an explicit Light palette independent of matchMedia", async () => {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: vi.fn(() => ({
        matches: true,
        media: "(prefers-color-scheme: dark)",
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    });
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    vi.mocked(ipc.setSetting).mockResolvedValue({
      theme: "light",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    const user = userEvent.setup();

    render(<AppearanceSection />);
    await user.click(await screen.findByRole("radio", { name: "Light" }));

    expect(document.documentElement.dataset.theme).toBe("light");
    expect(ipc.setSetting).toHaveBeenCalledWith("theme", "light");
    const styles = readCssBundle();
    const explicitLightRule = styles.match(
      /:root\[data-theme=["']light["']\]\s*\{([^}]*)\}/s,
    )?.[1];
    expect(explicitLightRule).toBeDefined();
    for (const token of ["--bg", "--panel", "--border", "--text", "--muted"]) {
      expect(explicitLightRule).toContain(`${token}:`);
    }
  });

  it("reverts to the previous theme and shows an error when persisting fails", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "light",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    vi.mocked(ipc.setSetting).mockRejectedValue(new Error("disk full"));
    const user = userEvent.setup();

    render(<AppearanceSection />);
    await user.click(await screen.findByRole("radio", { name: "Dark" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(/disk full/i);
    // Reverted back to the previously-applied theme (light), not left on
    // the failed selection (dark) and not silently cleared to system.
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(screen.getByRole("radio", { name: "Light" })).toBeChecked();
  });

  it("[WP-130] keeps the latest theme selected when an older persistence request rejects", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    let rejectDark!: (reason: Error) => void;
    vi.mocked(ipc.setSetting)
      .mockReturnValueOnce(
        new Promise((_, reject) => {
          rejectDark = reject;
        }),
      )
      .mockResolvedValueOnce({
        theme: "light",
        ui_language: "en",
        active_model_diarization: "none",
        export_file_type: "plain_text",
      });
    const user = userEvent.setup();
    render(<AppearanceSection />);

    await user.click(await screen.findByRole("radio", { name: "Dark" }));
    await user.click(screen.getByRole("radio", { name: "Light" }));
    rejectDark(new Error("older write failed"));

    expect(await screen.findByRole("radio", { name: "Light" })).toBeChecked();
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});
