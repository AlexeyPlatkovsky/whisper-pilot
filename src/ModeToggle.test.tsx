import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ModeToggle } from "./ModeToggle";

describe("ModeToggle user-facing workspace names", () => {
  it("labels the three modes Transcription, Meeting, and Recorder while preserving their callbacks", async () => {
    const selectTranscription = vi.fn();
    const selectMeeting = vi.fn();
    const selectRecorder = vi.fn();
    const user = userEvent.setup();
    render(
      <ModeToggle
        mode="streaming"
        onSelectMeeting={selectTranscription}
        onSelectStreaming={selectMeeting}
        onSelectRecorder={selectRecorder}
      />,
    );

    const group = screen.getByRole("group", { name: "Workspace mode" });
    const transcription = screen.getByRole("button", {
      name: "Transcription",
    });
    const meeting = screen.getByRole("button", { name: "Meeting" });
    const recorder = screen.getByRole("button", { name: "Recorder" });
    expect(group).toContainElement(transcription);
    expect(group).toContainElement(meeting);
    expect(group).toContainElement(recorder);
    expect(transcription).toHaveAttribute("aria-pressed", "false");
    expect(meeting).toHaveAttribute("aria-pressed", "true");
    expect(recorder).toHaveAttribute("aria-pressed", "false");

    await user.click(transcription);
    await user.click(meeting);
    await user.click(recorder);
    expect(selectTranscription).toHaveBeenCalledOnce();
    expect(selectMeeting).toHaveBeenCalledOnce();
    expect(selectRecorder).toHaveBeenCalledOnce();
  });
});
