import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ToggleSwitch } from "./ToggleSwitch";

// Shared pill switch (WP-96) used by both the Meeting and Streaming
// transcript headers to show/hide the MFU panel. Tested in isolation here;
// App.mfuToggle.test.tsx and StreamingView.mfuToggle.test.tsx cover its
// wiring into each screen.
describe("ToggleSwitch", () => {
  it("exposes role=switch with the given accessible name and an on checked state", () => {
    render(
      <ToggleSwitch checked={true} onChange={vi.fn()} label="MFU panel" />,
    );

    const el = screen.getByRole("switch", { name: "MFU panel" });

    expect(el).toHaveAttribute("aria-checked", "true");
  });

  it("exposes an off checked state", () => {
    render(
      <ToggleSwitch checked={false} onChange={vi.fn()} label="MFU panel" />,
    );

    expect(screen.getByRole("switch", { name: "MFU panel" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });

  it("calls onChange with the toggled value on click", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <ToggleSwitch checked={true} onChange={onChange} label="MFU panel" />,
    );

    await user.click(screen.getByRole("switch", { name: "MFU panel" }));

    expect(onChange).toHaveBeenCalledWith(false);
  });

  it("is keyboard-focusable and activates on Enter", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <ToggleSwitch checked={false} onChange={onChange} label="MFU panel" />,
    );

    await user.tab();
    expect(screen.getByRole("switch", { name: "MFU panel" })).toHaveFocus();
    await user.keyboard("{Enter}");

    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("activates on Space", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <ToggleSwitch checked={false} onChange={onChange} label="MFU panel" />,
    );

    await user.tab();
    await user.keyboard(" ");

    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("is enabled by default when no disabled prop is passed", () => {
    render(
      <ToggleSwitch checked={true} onChange={vi.fn()} label="MFU panel" />,
    );

    expect(
      screen.getByRole("switch", { name: "MFU panel" }),
    ).not.toBeDisabled();
  });

  // WP-93: opt-in disabled state (used by Live Translation, gated on model
  // readiness/Prettify state) — omitting the prop preserves the MFU
  // toggle's always-enabled behavior above.
  it("is disabled and exposes the reason as its title when disabled with a reason", () => {
    render(
      <ToggleSwitch
        checked={false}
        onChange={vi.fn()}
        label="Live Translation"
        disabled
        disabledReason="No language model is ready."
      />,
    );

    const el = screen.getByRole("switch", { name: "Live Translation" });
    expect(el).toBeDisabled();
    expect(el).toHaveAttribute("title", "No language model is ready.");
  });

  it("does not call onChange when clicked while disabled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <ToggleSwitch
        checked={false}
        onChange={onChange}
        label="Live Translation"
        disabled
        disabledReason="No language model is ready."
      />,
    );

    await user.click(screen.getByRole("switch", { name: "Live Translation" }));

    expect(onChange).not.toHaveBeenCalled();
  });

  it("uses the label as the title when enabled", () => {
    render(
      <ToggleSwitch checked={false} onChange={vi.fn()} label="MFU panel" />,
    );

    expect(screen.getByRole("switch", { name: "MFU panel" })).toHaveAttribute(
      "title",
      "MFU panel",
    );
  });
});
