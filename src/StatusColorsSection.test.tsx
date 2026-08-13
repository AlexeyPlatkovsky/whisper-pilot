import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StatusColorsSection } from "./StatusColorsSection";
import * as ipc from "./ipc";
import { DEFAULT_STATUS_COLORS, STATUS_COLOR_SPECS } from "./statusColors";

vi.mock("./ipc", () => ({
  setSetting: vi.fn(),
}));

afterEach(() => {
  vi.clearAllMocks();
  for (const spec of STATUS_COLOR_SPECS) {
    document.documentElement.style.removeProperty(`--status-color-${spec.key}`);
  }
});

function savedMapping(overrides: Record<string, string>): string {
  return JSON.stringify({ ...DEFAULT_STATUS_COLORS, ...overrides });
}

describe("StatusColorsSection — listing", () => {
  it("lists every current semantic status exactly once with its built-in hex", () => {
    render(<StatusColorsSection statusColorsRaw={undefined} />);

    for (const spec of STATUS_COLOR_SPECS) {
      expect(
        screen.getByRole("button", { name: new RegExp(spec.label) }),
      ).toBeInTheDocument();
    }
    expect(screen.getByText("#8A5F10")).toBeInTheDocument();
    expect(screen.getAllByText("#176C8F")).toHaveLength(4);
    expect(screen.getAllByText("#B82B2F")).toHaveLength(4);
  });

  it("lays the statuses out in two columns", () => {
    const { container } = render(
      <StatusColorsSection statusColorsRaw={undefined} />,
    );

    const columns = container.querySelectorAll(
      ".wp-status-color-grid > .wp-status-color-column",
    );
    expect(columns).toHaveLength(2);
    expect(columns[0].querySelectorAll(".wp-status-color-row")).toHaveLength(7);
    expect(columns[1].querySelectorAll(".wp-status-color-row")).toHaveLength(7);
  });

  it("sorts statuses alphabetically and fills each row from left to right", () => {
    const { container } = render(
      <StatusColorsSection statusColorsRaw={undefined} />,
    );
    const columns = container.querySelectorAll(
      ".wp-status-color-grid > .wp-status-color-column",
    );
    const alphabeticalLabels = [...STATUS_COLOR_SPECS]
      .sort((a, b) => a.label.localeCompare(b.label))
      .map((spec) => spec.label);
    const labelsIn = (column: Element) =>
      Array.from(
        column.querySelectorAll(".wp-status-color-name"),
        (label) => label.textContent,
      );

    expect(labelsIn(columns[0])).toEqual(
      alphabeticalLabels.filter((_, i) => i % 2 === 0),
    );
    expect(labelsIn(columns[1])).toEqual(
      alphabeticalLabels.filter((_, i) => i % 2 === 1),
    );
  });

  it("colors each status label with that status's current color", () => {
    render(<StatusColorsSection statusColorsRaw={undefined} />);

    expect(screen.getByText("No files")).toHaveStyle({ color: "#8A5F10" });
    expect(screen.getByText("Finished")).toHaveStyle({ color: "#46704C" });
  });

  it("shows no revert buttons while every color is the built-in default", () => {
    render(<StatusColorsSection statusColorsRaw={undefined} />);

    expect(
      screen.queryByRole("button", { name: /revert/i }),
    ).not.toBeInTheDocument();
  });
});

describe("StatusColorsSection — revert", () => {
  it("shows a revert button only on rows whose color differs from the default", () => {
    render(
      <StatusColorsSection
        statusColorsRaw={savedMapping({ ready: "#112233" })}
      />,
    );

    expect(
      screen.getByRole("button", { name: "Revert Ready to default color" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Revert Error to default color" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("#112233")).toBeInTheDocument();
  });

  it("reverts to the default color without asking for confirmation", async () => {
    vi.mocked(ipc.setSetting).mockResolvedValue({} as never);
    const user = userEvent.setup();
    render(
      <StatusColorsSection
        statusColorsRaw={savedMapping({ ready: "#112233" })}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "Revert Ready to default color" }),
    );

    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(ipc.setSetting).toHaveBeenCalledWith(
      "status_colors",
      savedMapping({}),
    );
    expect(
      document.documentElement.style.getPropertyValue("--status-color-ready"),
    ).toBe("#5A7684");
    expect(
      screen.queryByRole("button", { name: "Revert Ready to default color" }),
    ).not.toBeInTheDocument();
  });
});

describe("StatusColorsSection — picker popover", () => {
  async function openPicker(label: string) {
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: new RegExp(label) }));
    return { user, dialog: screen.getByRole("dialog") };
  }

  it("opens an anchored popover with the current color and closes on Cancel without persisting", async () => {
    vi.mocked(ipc.setSetting).mockResolvedValue({} as never);
    render(<StatusColorsSection statusColorsRaw={undefined} />);
    const { user, dialog } = await openPicker("Transcribing");

    expect(within(dialog).getByLabelText("Visual color picker")).toHaveValue(
      "#176c8f",
    );
    expect(within(dialog).getByLabelText("Hex color")).toHaveValue("#176C8F");

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(ipc.setSetting).not.toHaveBeenCalled();
  });

  it("closes on Escape without persisting", async () => {
    render(<StatusColorsSection statusColorsRaw={undefined} />);
    const { user } = await openPicker("Error");

    await user.keyboard("{Escape}");

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(ipc.setSetting).not.toHaveBeenCalled();
  });

  it("rejects a malformed hex value without changing the active or persisted mapping", async () => {
    render(<StatusColorsSection statusColorsRaw={undefined} />);
    const { user, dialog } = await openPicker("Error");

    const hexInput = within(dialog).getByLabelText("Hex color");
    await user.clear(hexInput);
    await user.type(hexInput, "#123");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(await within(dialog).findByRole("alert")).toBeInTheDocument();
    expect(ipc.setSetting).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(
      document.documentElement.style.getPropertyValue("--status-color-error"),
    ).toBe("");
  });

  it("saves a valid color, applies it immediately, and persists it", async () => {
    vi.mocked(ipc.setSetting).mockResolvedValue({} as never);
    render(<StatusColorsSection statusColorsRaw={undefined} />);
    const { user, dialog } = await openPicker("On Air");

    const hexInput = within(dialog).getByLabelText("Hex color");
    await user.clear(hexInput);
    await user.type(hexInput, "#334455");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(ipc.setSetting).toHaveBeenCalledWith(
      "status_colors",
      savedMapping({ "on-air": "#334455" }),
    );
    expect(
      document.documentElement.style.getPropertyValue("--status-color-on-air"),
    ).toBe("#334455");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByText("#334455")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Revert On Air to default color" }),
    ).toBeInTheDocument();
  });

  it("warns about low contrast inside the popover but still saves", async () => {
    vi.mocked(ipc.setSetting).mockResolvedValue({} as never);
    render(<StatusColorsSection statusColorsRaw={undefined} />);
    const { user, dialog } = await openPicker("Ready");

    const hexInput = within(dialog).getByLabelText("Hex color");
    await user.clear(hexInput);
    await user.type(hexInput, "#CCCCCC");

    expect(
      await within(dialog).findByText(
        "Low contrast (1.61<4.50:1) against the current theme background.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();

    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(ipc.setSetting).toHaveBeenCalledWith(
      "status_colors",
      savedMapping({ ready: "#CCCCCC" }),
    );
  });

  it("treats a ratio that rounds to 4.50 as passing", async () => {
    render(<StatusColorsSection statusColorsRaw={undefined} />);
    const { user, dialog } = await openPicker("Ready");

    const hexInput = within(dialog).getByLabelText("Hex color");
    await user.clear(hexInput);
    await user.type(hexInput, "#A96800");

    expect(within(dialog).queryByText(/low contrast/i)).not.toBeInTheDocument();
  });

  it("restores the prior mapping and shows an error when persisting fails", async () => {
    vi.mocked(ipc.setSetting).mockRejectedValue(new Error("disk full"));
    render(<StatusColorsSection statusColorsRaw={undefined} />);
    const { user, dialog } = await openPicker("Finished");

    const hexInput = within(dialog).getByLabelText("Hex color");
    await user.clear(hexInput);
    await user.type(hexInput, "#334455");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(/disk full/i);
    expect(
      document.documentElement.style.getPropertyValue(
        "--status-color-finished",
      ),
    ).toBe("#46704C");
    expect(screen.getByText("#46704C")).toBeInTheDocument();
  });

  it("treats a lowercase re-entry of the built-in hex as the default color", async () => {
    vi.mocked(ipc.setSetting).mockResolvedValue({} as never);
    render(<StatusColorsSection statusColorsRaw={undefined} />);
    const { user, dialog } = await openPicker("Error");

    const hexInput = within(dialog).getByLabelText("Hex color");
    await user.clear(hexInput);
    await user.type(hexInput, "#b82b2f");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(ipc.setSetting).toHaveBeenCalledWith(
      "status_colors",
      savedMapping({}),
    );
    expect(
      screen.queryByRole("button", { name: "Revert Error to default color" }),
    ).not.toBeInTheDocument();
  });

  it("ignores a stale persist failure after a newer action won", async () => {
    let rejectFirst: (e: Error) => void = () => {};
    vi.mocked(ipc.setSetting)
      .mockImplementationOnce(
        () =>
          new Promise((_, reject) => {
            rejectFirst = reject;
          }),
      )
      .mockResolvedValue({} as never);
    const user = userEvent.setup();
    render(<StatusColorsSection statusColorsRaw={undefined} />);

    // Action A: save a custom Error color; its persist stays pending.
    const dialog = await (async () => {
      await user.click(screen.getByRole("button", { name: /Error/ }));
      return screen.getByRole("dialog");
    })();
    const hexInput = within(dialog).getByLabelText("Hex color");
    await user.clear(hexInput);
    await user.type(hexInput, "#334455");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(
      document.documentElement.style.getPropertyValue("--status-color-error"),
    ).toBe("#334455");

    // Action B: Reset all, confirmed — the newer action determines the
    // final visible and persisted mapping.
    await user.click(screen.getByRole("button", { name: /reset all colors/i }));
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Reset all",
      }),
    );
    expect(
      document.documentElement.style.getPropertyValue("--status-color-error"),
    ).toBe("#B82B2F");

    // Action A's persist now fails late; the stale response must not revert
    // B's mapping nor surface an error.
    rejectFirst(new Error("disk full"));
    await new Promise((r) => setTimeout(r, 10));

    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      document.documentElement.style.getPropertyValue("--status-color-error"),
    ).toBe("#B82B2F");
    expect(screen.getByRole("button", { name: /Error/ })).toHaveTextContent(
      "#B82B2F",
    );
  });
});

describe("StatusColorsSection — reset all", () => {
  it("asks for confirmation before resetting", async () => {
    const user = userEvent.setup();
    render(
      <StatusColorsSection
        statusColorsRaw={savedMapping({ ready: "#112233" })}
      />,
    );

    await user.click(screen.getByRole("button", { name: /reset all colors/i }));

    const dialog = screen.getByRole("alertdialog");
    expect(dialog).toBeInTheDocument();
    expect(ipc.setSetting).not.toHaveBeenCalled();

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(ipc.setSetting).not.toHaveBeenCalled();
    expect(screen.getByText("#112233")).toBeInTheDocument();
  });

  it("confirming restores and persists the built-in mapping", async () => {
    vi.mocked(ipc.setSetting).mockResolvedValue({} as never);
    const user = userEvent.setup();
    render(
      <StatusColorsSection
        statusColorsRaw={savedMapping({ ready: "#112233" })}
      />,
    );

    await user.click(screen.getByRole("button", { name: /reset all colors/i }));
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Reset all",
      }),
    );

    expect(ipc.setSetting).toHaveBeenCalledWith(
      "status_colors",
      savedMapping({}),
    );
    expect(
      document.documentElement.style.getPropertyValue("--status-color-ready"),
    ).toBe("#5A7684");
    expect(screen.queryByText("#112233")).not.toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("restores the prior mapping and shows an error when the reset persist fails", async () => {
    vi.mocked(ipc.setSetting).mockRejectedValue(new Error("disk full"));
    const user = userEvent.setup();
    render(
      <StatusColorsSection
        statusColorsRaw={savedMapping({ ready: "#112233" })}
      />,
    );

    await user.click(screen.getByRole("button", { name: /reset all colors/i }));
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Reset all",
      }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(/disk full/i);
    expect(
      document.documentElement.style.getPropertyValue("--status-color-ready"),
    ).toBe("#112233");
    expect(screen.getByText("#112233")).toBeInTheDocument();
  });
});
