import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SpeakerLabelEditor } from "./SpeakerLabelEditor";

describe("SpeakerLabelEditor — disabled while a run is in flight", () => {
  it("force-closes an open rename input without committing the draft", async () => {
    const onRename = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(
      <SpeakerLabelEditor
        speakerId={1}
        label="Speaker 1"
        onRename={onRename}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Rename Speaker 1" }));
    const input = screen.getByRole("textbox", { name: "Rename Speaker 1" });
    await user.clear(input);
    await user.type(input, "Alex");

    // A run starts mid-edit — the input must close without saving "Alex".
    rerender(
      <SpeakerLabelEditor
        speakerId={1}
        label="Speaker 1"
        onRename={onRename}
        disabled
      />,
    );

    expect(onRename).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "Rename Speaker 1" }),
    ).toBeDisabled();
  });

  it("disables the closed rename button while a run is in flight", () => {
    render(
      <SpeakerLabelEditor
        speakerId={1}
        label="Speaker 1"
        onRename={vi.fn()}
        disabled
      />,
    );

    expect(
      screen.getByRole("button", { name: "Rename Speaker 1" }),
    ).toBeDisabled();
  });
});
