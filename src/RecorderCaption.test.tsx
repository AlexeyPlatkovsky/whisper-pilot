import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RecorderCaption } from "./RecorderCaption";
import * as ipc from "./ipc";

type Handler<T> = (payload: T) => void;
interface SessionEvent {
  id: number;
  status:
    "recording" | "finalizing" | "completed" | "recoverable" | "delete_failed";
}
interface SegmentEvent {
  session_id: number;
  text: string;
}
interface PartialEvent {
  session_id: number;
  revision: number;
  text: string;
}

let sessionHandler: Handler<SessionEvent> | null = null;
let committedHandler: Handler<SegmentEvent> | null = null;
let partialHandler: Handler<PartialEvent> | null = null;
let errorHandler: Handler<{ session_id?: number; message: string }> | null =
  null;
let sessionUnlisten: () => void;
let committedUnlisten: () => void;

vi.mock("./ipc", () => ({
  getLiveCaptureSnapshot: vi.fn(async () => ({
    phase: "capturing",
    session_id: 1,
    source: "recorder",
    generation: 1,
    revision: 1,
    error: null,
  })),
  getRecorderShortcutStatus: vi.fn(async () => ({
    configured: "Command+Shift+R",
    active: true,
  })),
  openRecorderSession: vi.fn(async () => ({
    id: 1,
    status: "recording",
    segments: [{ id: 10, text: "Hydrated phrase" }],
  })),
  showRecorderWorkspace: vi.fn(async () => {}),
  onRecorderSessionChanged: vi.fn(async (handler: Handler<SessionEvent>) => {
    sessionHandler = handler;
    return () => {};
  }),
  onRecorderSegmentCommitted: vi.fn(async (handler: Handler<SegmentEvent>) => {
    committedHandler = handler;
    return () => {};
  }),
  onRecorderPartial: vi.fn(async (handler: Handler<PartialEvent>) => {
    partialHandler = handler;
    return () => {};
  }),
  onRecorderError: vi.fn(async (handler) => {
    errorHandler = handler;
    return () => {};
  }),
}));

describe("Recorder compact caption", () => {
  beforeEach(() => {
    sessionHandler = null;
    committedHandler = null;
    partialHandler = null;
    errorHandler = null;
    sessionUnlisten = vi.fn();
    committedUnlisten = vi.fn();
    vi.clearAllMocks();
  });

  it("releases listeners registered before a later subscription rejects", async () => {
    vi.mocked(ipc.onRecorderSessionChanged).mockResolvedValueOnce(
      sessionUnlisten,
    );
    vi.mocked(ipc.onRecorderSegmentCommitted).mockResolvedValueOnce(
      committedUnlisten,
    );
    vi.mocked(ipc.onRecorderPartial).mockRejectedValueOnce(
      new Error("partial listener unavailable"),
    );

    const { unmount } = render(<RecorderCaption />);

    await waitFor(() => expect(ipc.onRecorderPartial).toHaveBeenCalledOnce());
    expect(sessionUnlisten).toHaveBeenCalledOnce();
    expect(committedUnlisten).toHaveBeenCalledOnce();
    unmount();
    expect(sessionUnlisten).toHaveBeenCalledOnce();
    expect(committedUnlisten).toHaveBeenCalledOnce();
  });

  it("hydrates the active note, shows the registered chord, and clears stale text for a new note", async () => {
    const user = userEvent.setup();
    render(<RecorderCaption />);

    expect(await screen.findByText("Hydrated phrase")).toBeInTheDocument();
    expect(screen.getByText("Command Shift R")).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Open Recorder workspace" }),
    );
    expect(ipc.showRecorderWorkspace).toHaveBeenCalledOnce();
    await waitFor(() => expect(sessionHandler).not.toBeNull());

    act(() => {
      sessionHandler?.({ id: 2, status: "recording" });
      partialHandler?.({ session_id: 1, revision: 50, text: "Old partial" });
      partialHandler?.({ session_id: 2, revision: 1, text: "New partial" });
    });
    expect(screen.queryByText("Hydrated phrase")).not.toBeInTheDocument();
    expect(screen.queryByText("Old partial")).not.toBeInTheDocument();
    expect(screen.getByText("New partial")).toBeInTheDocument();

    act(() => {
      committedHandler?.({ session_id: 2, text: "New committed phrase" });
    });
    expect(screen.getByText(/New committed phrase/)).toBeInTheDocument();
    expect(screen.queryByText("New partial")).not.toBeInTheDocument();
  });

  it("surfaces shortcut-status refresh failures without dropping the last chord", async () => {
    vi.mocked(ipc.getLiveCaptureSnapshot).mockResolvedValueOnce({
      phase: "idle",
      session_id: null,
      source: null,
      generation: 1,
      revision: 1,
      error: null,
    });
    vi.mocked(ipc.getRecorderShortcutStatus).mockRejectedValueOnce(
      new Error("Shortcut status IPC failed"),
    );
    render(<RecorderCaption />);

    expect(
      await screen.findByText("Shortcut status unavailable"),
    ).toBeInTheDocument();
    expect(screen.getByText(/Shortcut status IPC failed/)).toBeInTheDocument();
    expect(screen.getByText("Control Option Space")).toBeInTheDocument();
  });

  it("tracks only the active recording and reports only its errors", async () => {
    render(<RecorderCaption />);
    expect(await screen.findByText("Hydrated phrase")).toBeInTheDocument();
    await waitFor(() => expect(errorHandler).not.toBeNull());

    act(() => {
      sessionHandler?.({ id: 2, status: "recording" });
      committedHandler?.({ session_id: 1, text: "Wrong session" });
      errorHandler?.({ session_id: 1, message: "Wrong error" });
      sessionHandler?.({ id: 2, status: "finalizing" });
    });
    expect(screen.getByText("Finalizing")).toBeInTheDocument();
    expect(screen.queryByText("Wrong session")).not.toBeInTheDocument();
    expect(screen.queryByText("Wrong error")).not.toBeInTheDocument();

    act(() => {
      errorHandler?.({ session_id: 2, message: "Current error" });
    });
    expect(screen.getByText("Recorder error")).toBeInTheDocument();
    expect(screen.getByText("Current error")).toBeInTheDocument();
  });

  it("shows disabled shortcut detail and opens the workspace from Return", async () => {
    vi.mocked(ipc.getLiveCaptureSnapshot).mockResolvedValueOnce({
      phase: "idle",
      session_id: null,
      source: null,
      generation: 1,
      revision: 1,
      error: null,
    });
    vi.mocked(ipc.getRecorderShortcutStatus).mockResolvedValue({
      configured: "Command+Shift+R",
      active: false,
      error: "Registration conflict",
    });
    render(<RecorderCaption />);

    expect(await screen.findByText("Shortcut disabled")).toBeInTheDocument();
    expect(screen.getByText("Registration conflict")).toBeInTheDocument();
    const caption = screen.getByRole("button", {
      name: "Open Recorder workspace",
    });
    fireEvent.keyDown(caption, { key: "Enter" });
    expect(ipc.showRecorderWorkspace).toHaveBeenCalledOnce();
  });

  it("surfaces workspace-open failure", async () => {
    vi.mocked(ipc.showRecorderWorkspace).mockRejectedValueOnce(
      new Error("Window unavailable"),
    );
    const user = userEvent.setup();
    render(<RecorderCaption />);

    await user.click(
      await screen.findByRole("button", { name: "Open Recorder workspace" }),
    );
    expect(await screen.findByText("Recorder error")).toBeInTheDocument();
    expect(screen.getByText("Error: Window unavailable")).toBeInTheDocument();
  });
});
