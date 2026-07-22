import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AppearanceSection } from "./AppearanceSection";
import * as ipc from "./ipc";

vi.mock("./ipc", () => ({
  getSettings: vi.fn(),
  setSetting: vi.fn(),
}));

afterEach(() => {
  delete document.documentElement.dataset.theme;
});

describe("AppearanceSection", () => {
  it("shows an error when the initial theme load fails", async () => {
    vi.mocked(ipc.getSettings).mockRejectedValue(new Error("IPC unavailable"));

    render(<AppearanceSection />);

    expect(await screen.findByRole("alert")).toHaveTextContent(/IPC unavailable/i);
  });

  it("selects the currently persisted theme (System) by default", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({ theme: "system", ui_language: "en" });

    render(<AppearanceSection />);

    expect(await screen.findByRole("radio", { name: "System" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Light" })).not.toBeChecked();
    expect(screen.getByRole("radio", { name: "Dark" })).not.toBeChecked();
  });

  it("selects Dark by default when that is the persisted theme", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({ theme: "dark", ui_language: "en" });

    render(<AppearanceSection />);

    expect(await screen.findByRole("radio", { name: "Dark" })).toBeChecked();
  });

  it("clicking Dark applies it immediately and persists it", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({ theme: "system", ui_language: "en" });
    vi.mocked(ipc.setSetting).mockResolvedValue({ theme: "dark", ui_language: "en" });
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
    vi.mocked(ipc.getSettings).mockResolvedValue({ theme: "dark", ui_language: "en" });
    vi.mocked(ipc.setSetting).mockResolvedValue({ theme: "system", ui_language: "en" });
    const user = userEvent.setup();

    render(<AppearanceSection />);
    await user.click(await screen.findByRole("radio", { name: "System" }));

    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it("reverts to the previous theme and shows an error when persisting fails", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({ theme: "light", ui_language: "en" });
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
});
