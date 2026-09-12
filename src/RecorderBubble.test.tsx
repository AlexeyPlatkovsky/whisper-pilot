import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  RecorderBubble,
  bubblePresentation,
  type BubbleWindowAdapter,
} from "./RecorderBubble";

function adapter(): BubbleWindowAdapter {
  return {
    startDragging: vi.fn().mockResolvedValue(undefined),
    restoreMainWindow: vi.fn().mockResolvedValue(undefined),
  };
}

describe("RecorderBubble", () => {
  it("communicates idle, listening and actionable error without color", () => {
    expect(bubblePresentation({ phase: "idle", source: null }, null)).toEqual({
      kind: "idle",
      label: "WhisperPilot idle",
      symbol: "pause",
    });
    expect(
      bubblePresentation({ phase: "capturing", source: "recorder" }, null),
    ).toEqual({
      kind: "listening",
      label: "WhisperPilot listening to microphone",
      symbol: "mic",
    });
    expect(
      bubblePresentation(
        { phase: "capturing", source: "streaming" },
        "Audio device disconnected",
      ),
    ).toEqual({
      kind: "error",
      label: "WhisperPilot error: Audio device disconnected",
      symbol: "alert",
    });
  });

  it("restores through semantic click activation", () => {
    const windowAdapter = adapter();
    render(<RecorderBubble windowAdapter={windowAdapter} />);

    fireEvent.click(screen.getByRole("button", { name: /WhisperPilot idle/i }));

    expect(windowAdapter.restoreMainWindow).toHaveBeenCalledTimes(1);
  });

  it("restores after a click gesture but suppresses the synthetic click after drag", () => {
    const windowAdapter = adapter();
    render(<RecorderBubble windowAdapter={windowAdapter} />);
    const bubble = screen.getByRole("button", { name: /WhisperPilot idle/i });

    fireEvent.pointerDown(bubble, { clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerUp(bubble, { clientX: 12, clientY: 13, pointerId: 1 });
    fireEvent.click(bubble);
    expect(windowAdapter.restoreMainWindow).toHaveBeenCalledTimes(1);

    fireEvent.pointerDown(bubble, { clientX: 10, clientY: 10, pointerId: 2 });
    fireEvent.pointerMove(bubble, { clientX: 30, clientY: 10, pointerId: 2 });
    fireEvent.pointerUp(bubble, { clientX: 30, clientY: 10, pointerId: 2 });
    fireEvent.click(bubble);

    expect(windowAdapter.startDragging).toHaveBeenCalledTimes(1);
    expect(windowAdapter.restoreMainWindow).toHaveBeenCalledTimes(1);
  });
});
