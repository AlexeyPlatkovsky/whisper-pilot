import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StreamingView } from "./StreamingView";
import * as ipc from "./ipc";
import type { StreamingSession, StreamingSessionSummary } from "./ipc";

type Handler<T> = (payload: T) => void;

let windowHandler: Handler<
  ipc.StreamingWindow & { session_id: number }
> | null = null;
let sourcesHandler: Handler<ipc.StreamingSources> | null = null;
let endedHandler: Handler<{ session_id: number }> | null = null;

vi.mock("./ipc", () => ({
  listStreamingSessions: vi.fn(async () => []),
  openStreamingSession: vi.fn(),
  renameStreamingSession: vi.fn(),
  deleteStreamingSession: vi.fn(),
  startStreamingSession: vi.fn(),
  stopStreamingSession: vi.fn(),
  onStreamingWindow: vi.fn(async (handler: Handler<unknown>) => {
    windowHandler = handler as Handler<
      ipc.StreamingWindow & { session_id: number }
    >;
    return () => {
      windowHandler = null;
    };
  }),
  onStreamingSources: vi.fn(async (handler: Handler<unknown>) => {
    sourcesHandler = handler as Handler<ipc.StreamingSources>;
    return () => {
      sourcesHandler = null;
    };
  }),
  onStreamingSessionEnded: vi.fn(async (handler: Handler<unknown>) => {
    endedHandler = handler as Handler<{ session_id: number }>;
    return () => {
      endedHandler = null;
    };
  }),
}));

const SESSION_A: StreamingSessionSummary = {
  id: 1,
  title: "Standup",
  created_at_ms: 100,
  updated_at_ms: 100,
  status: "stopped",
};

function openedSession(
  overrides: Partial<StreamingSession> = {},
): StreamingSession {
  return {
    id: 1,
    title: "Standup",
    created_at_ms: 100,
    updated_at_ms: 100,
    status: "stopped",
    windows: [],
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  windowHandler = null;
  sourcesHandler = null;
  endedHandler = null;
  vi.mocked(ipc.listStreamingSessions).mockResolvedValue([]);
});

describe("StreamingView", () => {
  it("lists persisted sessions on mount", async () => {
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);

    render(<StreamingView onClose={vi.fn()} />);

    expect(await screen.findByText("Standup")).toBeInTheDocument();
  });

  it("shows the empty state before any session is started or opened", async () => {
    render(<StreamingView onClose={vi.fn()} />);

    expect(
      await screen.findByText(/Start a session, or open one/),
    ).toBeInTheDocument();
  });

  it("starting a session shows Stop and the Listening state", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    render(<StreamingView onClose={vi.fn()} />);

    await user.click(await screen.findByRole("button", { name: "Start" }));

    expect(
      await screen.findByRole("button", { name: "Stop" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Listening…")).toBeInTheDocument();
  });

  it("appends a live window for the active session as it arrives", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());

    windowHandler!({
      session_id: 2,
      window_index: 0,
      start_ms: 0,
      end_ms: 7000,
      text: "hello there",
      language: "en",
      outcome_ok: true,
    });

    expect(await screen.findByText(/hello there/)).toBeInTheDocument();
  });

  it("ignores a live window for a session that is not the active one", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());

    windowHandler!({
      session_id: 999, // a different, stale session
      window_index: 0,
      start_ms: 0,
      end_ms: 7000,
      text: "should not appear",
      language: "en",
      outcome_ok: true,
    });

    expect(screen.queryByText(/should not appear/)).not.toBeInTheDocument();
  });

  it("a later window with the same index replaces the earlier one, not duplicates it", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());

    windowHandler!({
      session_id: 2,
      window_index: 0,
      start_ms: 0,
      end_ms: 7000,
      text: "first pass",
      language: "en",
      outcome_ok: true,
    });
    windowHandler!({
      session_id: 2,
      window_index: 0,
      start_ms: 0,
      end_ms: 7000,
      text: "corrected pass",
      language: "en",
      outcome_ok: true,
    });

    expect(await screen.findByText(/corrected pass/)).toBeInTheDocument();
    expect(screen.queryByText(/first pass/)).not.toBeInTheDocument();
  });

  it("a fail-open window renders as unavailable, not empty silence", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());

    windowHandler!({
      session_id: 2,
      window_index: 0,
      start_ms: 0,
      end_ms: 7000,
      text: "",
      language: "auto",
      outcome_ok: false,
    });

    expect(await screen.findByText("[unavailable]")).toBeInTheDocument();
  });

  it("shows the active audio source(s) once reported", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(sourcesHandler).not.toBeNull());

    sourcesHandler!({ session_id: 2, mic: true, system_audio: false });

    expect(await screen.findByText("Mic only")).toBeInTheDocument();
  });

  it("stopping flips back to Start once streaming_session_ended fires", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    vi.mocked(ipc.stopStreamingSession).mockResolvedValue(undefined);
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(endedHandler).not.toBeNull());

    await user.click(await screen.findByRole("button", { name: "Stop" }));
    expect(ipc.stopStreamingSession).toHaveBeenCalled();

    // The backend, not the Stop call returning, is the source of truth.
    endedHandler!({ session_id: 2 });

    expect(
      await screen.findByRole("button", { name: "Start" }),
    ).toBeInTheDocument();
  });

  it("a failed start surfaces the error instead of flipping to Stop", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockRejectedValue(
      "a meeting is currently transcribing",
    );
    render(<StreamingView onClose={vi.fn()} />);

    await user.click(await screen.findByRole("button", { name: "Start" }));

    expect(
      await screen.findByText(/a meeting is currently transcribing/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start" })).toBeInTheDocument();
  });

  it("opening a past session loads its stored windows and is not running", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.openStreamingSession).mockResolvedValue(
      openedSession({
        windows: [
          {
            window_index: 0,
            start_ms: 0,
            end_ms: 7000,
            text: "past meeting notes",
            language: "en",
            outcome_ok: true,
          },
        ],
      }),
    );
    render(<StreamingView onClose={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));

    expect(await screen.findByText(/past meeting notes/)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Stop" }),
    ).not.toBeInTheDocument();
  });

  it("renaming a session calls the IPC command with the new title and refreshes the list", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.renameStreamingSession).mockResolvedValue(
      openedSession({ title: "Renamed" }),
    );
    vi.spyOn(window, "prompt").mockReturnValue("Renamed");
    render(<StreamingView onClose={vi.fn()} />);
    await screen.findByText("Standup");

    await user.click(screen.getByRole("button", { name: "Rename Standup" }));

    expect(ipc.renameStreamingSession).toHaveBeenCalledWith(1, "Renamed");
  });

  it("declining the rename prompt makes no IPC call", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.spyOn(window, "prompt").mockReturnValue(null);
    render(<StreamingView onClose={vi.fn()} />);
    await screen.findByText("Standup");

    await user.click(screen.getByRole("button", { name: "Rename Standup" }));

    expect(ipc.renameStreamingSession).not.toHaveBeenCalled();
  });

  it("deleting a session asks for confirmation and calls the IPC command", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.deleteStreamingSession).mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<StreamingView onClose={vi.fn()} />);
    await screen.findByText("Standup");

    await user.click(screen.getByRole("button", { name: "Delete Standup" }));

    expect(ipc.deleteStreamingSession).toHaveBeenCalledWith(1);
  });

  it("declining the delete confirmation makes no IPC call", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<StreamingView onClose={vi.fn()} />);
    await screen.findByText("Standup");

    await user.click(screen.getByRole("button", { name: "Delete Standup" }));

    expect(ipc.deleteStreamingSession).not.toHaveBeenCalled();
  });

  it("calls onClose when the close button is clicked", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<StreamingView onClose={onClose} />);

    await user.click(
      await screen.findByRole("button", { name: "Back to Meetings" }),
    );

    expect(onClose).toHaveBeenCalled();
  });

  it("unlistens all three event handlers on unmount", async () => {
    const { unmount } = render(<StreamingView onClose={vi.fn()} />);
    await waitFor(() => expect(windowHandler).not.toBeNull());

    unmount();

    expect(windowHandler).toBeNull();
    expect(sourcesHandler).toBeNull();
    expect(endedHandler).toBeNull();
  });

  it("unmounting before event registration resolves still unlistens once it does (no leaked listeners)", async () => {
    const { unmount } = render(<StreamingView onClose={vi.fn()} />);

    // Unmount immediately, before the registration Promise.all has settled —
    // the cancelled-cleanup branch, not the steady-state one above.
    unmount();
    await waitFor(() => expect(windowHandler).not.toBeNull());

    // The handlers were registered (the mocked listen calls always resolve),
    // but the component's own cleanup ran before they were stored, so it
    // must unlisten them itself once they arrive rather than leaking them.
    expect(windowHandler).toBeNull();
    expect(sourcesHandler).toBeNull();
    expect(endedHandler).toBeNull();
  });

  it("reports mixed mic + system audio sources", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(sourcesHandler).not.toBeNull());

    sourcesHandler!({ session_id: 2, mic: true, system_audio: true });

    expect(await screen.findByText("Mic + System audio")).toBeInTheDocument();
  });

  it("reports system-audio-only sources", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(sourcesHandler).not.toBeNull());

    sourcesHandler!({ session_id: 2, mic: false, system_audio: true });

    expect(await screen.findByText("System audio only")).toBeInTheDocument();
  });

  it("reports no audio source when neither came up", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(sourcesHandler).not.toBeNull());

    sourcesHandler!({ session_id: 2, mic: false, system_audio: false });

    expect(await screen.findByText("No audio source")).toBeInTheDocument();
  });

  it("a failed Stop call surfaces the error", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    vi.mocked(ipc.stopStreamingSession).mockRejectedValue(
      "capture is not responding",
    );
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));

    await user.click(await screen.findByRole("button", { name: "Stop" }));

    expect(
      await screen.findByText(/capture is not responding/),
    ).toBeInTheDocument();
  });

  it("a failed open surfaces the error", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.openStreamingSession).mockRejectedValue(
      "streaming session 1 was not found",
    );
    render(<StreamingView onClose={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));

    expect(
      await screen.findByText(/streaming session 1 was not found/),
    ).toBeInTheDocument();
  });

  it("a failed rename surfaces the error", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.renameStreamingSession).mockRejectedValue("rename failed");
    vi.spyOn(window, "prompt").mockReturnValue("Renamed");
    render(<StreamingView onClose={vi.fn()} />);
    await screen.findByText("Standup");

    await user.click(screen.getByRole("button", { name: "Rename Standup" }));

    expect(await screen.findByText(/rename failed/)).toBeInTheDocument();
  });

  it("a failed delete surfaces the error", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.deleteStreamingSession).mockRejectedValue("delete failed");
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<StreamingView onClose={vi.fn()} />);
    await screen.findByText("Standup");

    await user.click(screen.getByRole("button", { name: "Delete Standup" }));

    expect(await screen.findByText(/delete failed/)).toBeInTheDocument();
  });

  it("deleting the currently open session clears the transcript view", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.openStreamingSession).mockResolvedValue(
      openedSession({
        windows: [
          {
            window_index: 0,
            start_ms: 0,
            end_ms: 7000,
            text: "notes to be cleared",
            language: "en",
            outcome_ok: true,
          },
        ],
      }),
    );
    vi.mocked(ipc.deleteStreamingSession).mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));
    await screen.findByText(/notes to be cleared/);
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([]);

    await user.click(screen.getByRole("button", { name: "Delete Standup" }));

    expect(screen.queryByText(/notes to be cleared/)).not.toBeInTheDocument();
    expect(
      await screen.findByText(/Start a session, or open one/),
    ).toBeInTheDocument();
  });
});
