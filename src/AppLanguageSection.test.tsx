import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { AppLanguageSection } from "./AppLanguageSection";
import * as ipc from "./ipc";

vi.mock("./ipc", () => ({
  getSettings: vi.fn(),
}));

describe("AppLanguageSection", () => {
  it("shows English checked as the current UI language", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({ theme: "system", ui_language: "en" });

    render(<AppLanguageSection />);

    expect(await screen.findByRole("radio", { name: "English" })).toBeChecked();
    // English is the only selectable UI language in beta (F005-R8).
    expect(screen.getAllByRole("radio")).toHaveLength(1);
  });

  it("shows an error when the settings load fails", async () => {
    vi.mocked(ipc.getSettings).mockRejectedValue(new Error("IPC unavailable"));

    render(<AppLanguageSection />);

    expect(await screen.findByRole("alert")).toHaveTextContent(/IPC unavailable/i);
  });
});
