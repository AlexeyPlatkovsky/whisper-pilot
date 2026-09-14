import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  getLiveCaptureSnapshotMock,
  onLiveCaptureStateMock,
  restoreMainFromBubbleMock,
  startDraggingMock,
  unlistenMock,
} = vi.hoisted(() => ({
  getLiveCaptureSnapshotMock: vi.fn(),
  onLiveCaptureStateMock: vi.fn(),
  restoreMainFromBubbleMock: vi.fn(),
  startDraggingMock: vi.fn(),
  unlistenMock: vi.fn(),
}));

vi.mock("./ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./ipc")>()),
  getLiveCaptureSnapshot: getLiveCaptureSnapshotMock,
  onLiveCaptureState: onLiveCaptureStateMock,
  restoreMainFromBubble: restoreMainFromBubbleMock,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ startDragging: startDraggingMock }),
}));
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
  beforeEach(() => {
    vi.clearAllMocks();
    getLiveCaptureSnapshotMock.mockResolvedValue({
      phase: "capturing",
      source: "recorder",
      session_id: 7,
      generation: 1,
      revision: 1,
      error: null,
    });
    onLiveCaptureStateMock.mockResolvedValue(unlistenMock);
    restoreMainFromBubbleMock.mockResolvedValue(undefined);
    startDraggingMock.mockResolvedValue(undefined);
    document.documentElement.classList.remove("bubble-document");
    document.body.classList.remove("bubble-document");
  });

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

  it("hydrates and follows the native live state, then releases the listener and document classes", async () => {
    let liveHandler: ((snapshot: Record<string, unknown>) => void) | undefined;
    onLiveCaptureStateMock.mockImplementation(async (handler) => {
      liveHandler = handler;
      return unlistenMock;
    });
    const { unmount } = render(<RecorderBubble />);

    expect(
      await screen.findByRole("button", {
        name: "WhisperPilot listening to microphone",
      }),
    ).toBeInTheDocument();
    expect(document.documentElement).toHaveClass("bubble-document");
    expect(document.body).toHaveClass("bubble-document");

    act(() => {
      liveHandler?.({
        phase: "capturing",
        source: "streaming",
        error: "Device disconnected",
      });
    });
    expect(
      screen.getByRole("button", {
        name: "WhisperPilot error: Device disconnected",
      }),
    ).toBeInTheDocument();

    unmount();
    expect(unlistenMock).toHaveBeenCalledOnce();
    expect(document.documentElement).not.toHaveClass("bubble-document");
    expect(document.body).not.toHaveClass("bubble-document");
  });

  it("surfaces native hydration, restore, and drag failures", async () => {
    getLiveCaptureSnapshotMock.mockRejectedValueOnce(
      new Error("Snapshot failed"),
    );
    restoreMainFromBubbleMock.mockRejectedValueOnce(
      new Error("Restore failed"),
    );
    startDraggingMock.mockRejectedValueOnce(new Error("Drag failed"));
    render(<RecorderBubble />);

    expect(
      await screen.findByRole("button", {
        name: /WhisperPilot error: Error: Snapshot failed/,
      }),
    ).toBeInTheDocument();
    const bubble = screen.getByRole("button");
    fireEvent.click(bubble);
    expect(
      await screen.findByRole("button", {
        name: /WhisperPilot error: Error: Restore failed/,
      }),
    ).toBeInTheDocument();

    fireEvent.pointerDown(bubble, { clientX: 0, clientY: 0, pointerId: 3 });
    fireEvent.pointerMove(bubble, { clientX: 20, clientY: 0, pointerId: 3 });
    fireEvent.pointerMove(bubble, { clientX: 30, clientY: 0, pointerId: 3 });
    expect(
      await screen.findByRole("button", {
        name: /WhisperPilot error: Error: Drag failed/,
      }),
    ).toBeInTheDocument();
  });

  it("cancels an abandoned pointer gesture and permits keyboard activation after a drag", async () => {
    render(<RecorderBubble />);
    const bubble = await screen.findByRole("button");

    fireEvent.pointerDown(bubble, { clientX: 0, clientY: 0, pointerId: 4 });
    fireEvent.pointerCancel(bubble, { pointerId: 4 });
    fireEvent.pointerMove(bubble, { clientX: 20, clientY: 0, pointerId: 4 });
    expect(startDraggingMock).not.toHaveBeenCalled();

    fireEvent.pointerDown(bubble, { clientX: 0, clientY: 0, pointerId: 5 });
    fireEvent.pointerMove(bubble, { clientX: 20, clientY: 0, pointerId: 5 });
    fireEvent.keyDown(bubble, { key: "Enter" });
    fireEvent.click(bubble);

    await waitFor(() =>
      expect(restoreMainFromBubbleMock).toHaveBeenCalledOnce(),
    );
  });
});
