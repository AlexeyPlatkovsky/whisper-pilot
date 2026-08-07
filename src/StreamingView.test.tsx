import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StreamingView } from "./StreamingView";
import * as ipc from "./ipc";
import type {
  StreamingNotes,
  StreamingSession,
  StreamingSessionSummary,
} from "./ipc";

type Handler<T> = (payload: T) => void;

let windowHandler: Handler<
  ipc.StreamingWindow & { session_id: number }
> | null = null;
let sourcesHandler: Handler<ipc.StreamingSources> | null = null;
let endedHandler: Handler<{ session_id: number }> | null = null;
let writeTextMock: ReturnType<typeof vi.spyOn>;

vi.mock("./ipc", () => ({
  listStreamingSessions: vi.fn(async () => []),
  openStreamingSession: vi.fn(),
  renameStreamingSession: vi.fn(),
  deleteStreamingSession: vi.fn(),
  startStreamingSession: vi.fn(),
  stopStreamingSession: vi.fn(),
  generateStreamingNotes: vi.fn(),
  generateStreamingPrettify: vi.fn(),
  acceptStreamingPrettify: vi.fn(),
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
  saveTextDialog: vi.fn(async () => null),
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

const NOTES: StreamingNotes = {
  summary: "Discussed Q3 roadmap.",
  decisions: "Ship M1 by Friday.",
  action_items: "Alex: update deck",
  open_questions: "Budget for Q4?",
  participants: "Alex, Sam",
};

const ONE_WINDOW = [
  {
    window_index: 0,
    start_ms: 0,
    end_ms: 7000,
    text: "hello there",
    language: "en",
    outcome_ok: true,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  windowHandler = null;
  sourcesHandler = null;
  endedHandler = null;
  vi.mocked(ipc.listStreamingSessions).mockResolvedValue([]);
  // jsdom provides a real, functional Clipboard implementation on a
  // non-configurable `navigator`, so replacing `navigator`/`navigator.
  // clipboard` wholesale (Object.assign, defineProperty, vi.stubGlobal) is
  // silently ineffective — spying on the real method is what actually
  // intercepts the call. Whether jsdom has initialized `navigator.clipboard`
  // by this point varies with run order/isolation, so fall back to defining
  // a plain stub object to spy on when it's not there yet.
  if (!navigator.clipboard) {
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: async () => {} },
      configurable: true,
    });
  }
  writeTextMock = vi
    .spyOn(navigator.clipboard, "writeText")
    .mockResolvedValue(undefined);
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

  it("Copy and Export are disabled until there is transcript text", async () => {
    render(<StreamingView onClose={vi.fn()} />);
    await screen.findByText(/Start a session, or open one/);

    expect(
      screen.getByRole("button", { name: "Copy transcript" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Export as Markdown" }),
    ).toBeDisabled();
  });

  it("Copy writes the plain transcript to the clipboard, marking [unavailable] windows too", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.openStreamingSession).mockResolvedValue(
      openedSession({
        windows: [
          {
            window_index: 0,
            start_ms: 0,
            end_ms: 7000,
            text: "hello",
            language: "en",
            outcome_ok: true,
          },
          {
            window_index: 1,
            start_ms: 7000,
            end_ms: 14000,
            text: "",
            language: "auto",
            outcome_ok: false,
          },
        ],
      }),
    );
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));

    await user.click(
      await screen.findByRole("button", { name: "Copy transcript" }),
    );

    expect(await screen.findByText("Copied")).toBeInTheDocument();
    expect(writeTextMock).toHaveBeenCalledWith("hello [unavailable]");
  });

  it("Export saves a Markdown-formatted file named after the session title", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.openStreamingSession).mockResolvedValue(
      openedSession({
        title: "Team Standup",
        windows: [
          {
            window_index: 0,
            start_ms: 0,
            end_ms: 7000,
            text: "hello there",
            language: "en",
            outcome_ok: true,
          },
        ],
      }),
    );
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));

    await user.click(
      await screen.findByRole("button", { name: "Export as Markdown" }),
    );

    expect(ipc.saveTextDialog).toHaveBeenCalledWith(
      "# Team Standup\n\nhello there\n",
      "Team Standup.md",
    );
  });

  it("a failed clipboard write surfaces the error instead of silently doing nothing", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.openStreamingSession).mockResolvedValue(
      openedSession({
        windows: [
          {
            window_index: 0,
            start_ms: 0,
            end_ms: 7000,
            text: "hello",
            language: "en",
            outcome_ok: true,
          },
        ],
      }),
    );
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue("denied");
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));

    await user.click(
      await screen.findByRole("button", { name: "Copy transcript" }),
    );

    expect(await screen.findByText(/denied/)).toBeInTheDocument();
  });

  it("a failed export surfaces the error", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.openStreamingSession).mockResolvedValue(
      openedSession({
        windows: [
          {
            window_index: 0,
            start_ms: 0,
            end_ms: 7000,
            text: "hello",
            language: "en",
            outcome_ok: true,
          },
        ],
      }),
    );
    vi.mocked(ipc.saveTextDialog).mockRejectedValue("disk full");
    render(<StreamingView onClose={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));

    await user.click(
      await screen.findByRole("button", { name: "Export as Markdown" }),
    );

    expect(await screen.findByText(/disk full/)).toBeInTheDocument();
  });

  describe("status widget", () => {
    // S-2: first-launch / empty state
    it("shows Ready with no timer before any session has run", async () => {
      render(<StreamingView onClose={vi.fn()} />);
      await screen.findByText(/Start a session, or open one/);

      const status = await screen.findByRole("status");
      expect(status).toHaveTextContent("Ready");
      expect(status.querySelector(".wp-status-timer")).toBeNull();
    });

    // S-1 + S-4: full state cycle, including the Starting transitional state
    it("cycles Ready -> Starting… -> On Air -> Ready across a full session", async () => {
      const user = userEvent.setup();
      let resolveStart!: (v: {
        id: number;
        title: string;
        created_at_ms: number;
        updated_at_ms: number;
        status: string;
      }) => void;
      vi.mocked(ipc.startStreamingSession).mockReturnValue(
        new Promise((resolve) => {
          resolveStart = resolve;
        }),
      );
      render(<StreamingView onClose={vi.fn()} />);
      const status = await screen.findByRole("status");
      expect(status).toHaveTextContent("Ready");

      await user.click(await screen.findByRole("button", { name: "Start" }));
      expect(status).toHaveTextContent("Starting…");

      resolveStart({
        id: 2,
        title: "New Streaming Session",
        created_at_ms: 200,
        updated_at_ms: 200,
        status: "active",
      });
      await waitFor(() => expect(status).toHaveTextContent("On Air"));
      expect(status.querySelector(".wp-status-timer")?.textContent).toBe(
        "00:00",
      );

      vi.mocked(ipc.stopStreamingSession).mockResolvedValue(undefined);
      await user.click(await screen.findByRole("button", { name: "Stop" }));
      endedHandler!({ session_id: 2 });

      await waitFor(() => expect(status).toHaveTextContent("Ready"));
      expect(status.querySelector(".wp-status-timer")).toBeNull();
    });

    // S-3: the timer switches to h:mm:ss once elapsed time reaches an hour
    it("switches the timer to h:mm:ss format once elapsed time reaches 60 minutes", async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      try {
        const user = userEvent.setup({
          advanceTimers: vi.advanceTimersByTime.bind(vi),
        });
        vi.mocked(ipc.startStreamingSession).mockResolvedValue({
          id: 2,
          title: "New Streaming Session",
          created_at_ms: 200,
          updated_at_ms: 200,
          status: "active",
        });
        render(<StreamingView onClose={vi.fn()} />);
        const status = await screen.findByRole("status");

        await user.click(await screen.findByRole("button", { name: "Start" }));
        await waitFor(() => expect(status).toHaveTextContent("On Air"));

        await vi.advanceTimersByTimeAsync(59 * 60 * 1000 + 58 * 1000);
        expect(status.querySelector(".wp-status-timer")?.textContent).toBe(
          "59:58",
        );

        await vi.advanceTimersByTimeAsync(3000);
        expect(status.querySelector(".wp-status-timer")?.textContent).toBe(
          "1:00:01",
        );
      } finally {
        vi.useRealTimers();
      }
    });

    // S-5: the session ending on its own (not via manual Stop) still resets the widget
    it("returns to Ready when the session ends on its own, not only via manual Stop", async () => {
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 2,
        title: "New Streaming Session",
        created_at_ms: 200,
        updated_at_ms: 200,
        status: "active",
      });
      const user = userEvent.setup();
      render(<StreamingView onClose={vi.fn()} />);
      const status = await screen.findByRole("status");
      await user.click(await screen.findByRole("button", { name: "Start" }));
      await waitFor(() => expect(status).toHaveTextContent("On Air"));

      endedHandler!({ session_id: 2 });

      await waitFor(() => expect(status).toHaveTextContent("Ready"));
    });

    // S-6: opening a past stopped session shows Ready, never On Air
    it("shows Ready, not On Air, when opening a past stopped session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(openedSession());
      render(<StreamingView onClose={vi.fn()} />);
      const status = await screen.findByRole("status");

      await user.click(await screen.findByText("Standup"));

      await waitFor(() => expect(status).toHaveTextContent("Ready"));
    });

    // S-7: a Start call that fails before isRunning ever becomes true falls
    // back to Ready, with the failure shown only in the existing error banner
    it("falls back to Ready when the Start call fails, showing the error separately", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.startStreamingSession).mockRejectedValue(
        "a meeting is currently transcribing",
      );
      render(<StreamingView onClose={vi.fn()} />);
      const status = await screen.findByRole("status");

      await user.click(await screen.findByRole("button", { name: "Start" }));

      expect(
        await screen.findByText(/a meeting is currently transcribing/),
      ).toBeInTheDocument();
      await waitFor(() => expect(status).toHaveTextContent("Ready"));
      expect(status).not.toHaveTextContent(
        "a meeting is currently transcribing",
      );
    });

    // S-8: a Stop request in flight keeps the widget on On Air until
    // isRunning actually flips false via streaming_session_ended
    it("stays On Air while a Stop request is in flight", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 2,
        title: "New Streaming Session",
        created_at_ms: 200,
        updated_at_ms: 200,
        status: "active",
      });
      let resolveStop!: () => void;
      vi.mocked(ipc.stopStreamingSession).mockReturnValue(
        new Promise((resolve) => {
          resolveStop = resolve;
        }),
      );
      render(<StreamingView onClose={vi.fn()} />);
      const status = await screen.findByRole("status");
      await user.click(await screen.findByRole("button", { name: "Start" }));
      await waitFor(() => expect(status).toHaveTextContent("On Air"));

      await user.click(await screen.findByRole("button", { name: "Stop" }));
      expect(status).toHaveTextContent("On Air");

      resolveStop();
      endedHandler!({ session_id: 2 });
      await waitFor(() => expect(status).toHaveTextContent("Ready"));
    });
  });

  describe("Craft / MFU", () => {
    // S-3 + S-4: disabled with no text, and while running
    it("disables Craft with no decoded text, enables it once a stopped session has text", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: [] }),
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      expect(
        screen.getByRole("button", { name: "Craft MFU notes" }),
      ).toBeDisabled();

      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      await user.click(await screen.findByText("Standup"));

      expect(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      ).not.toBeDisabled();
    });

    // EP: a fail-open-only session has display text ("[unavailable]") but no
    // real decoded content — Craft must stay disabled even though Copy/
    // Export's hasText guard would read true for the same windows.
    it("disables Craft when the session has only fail-open windows", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({
          windows: [
            {
              window_index: 0,
              start_ms: 0,
              end_ms: 7000,
              text: "",
              language: "auto",
              outcome_ok: false,
            },
          ],
        }),
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      expect(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      ).toBeDisabled();
    });

    it("disables Craft while the session is running, even with decoded text", async () => {
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
      windowHandler!({ ...ONE_WINDOW[0], session_id: 2 });

      expect(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      ).toBeDisabled();
    });

    // S-1: happy path
    it("shows the placeholder before Craft, then the generated notes after", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      let resolveCraft!: (v: StreamingSession) => void;
      vi.mocked(ipc.generateStreamingNotes).mockReturnValue(
        new Promise((resolve) => {
          resolveCraft = resolve;
        }),
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      expect(screen.getByText("Run MFU Craft")).toBeInTheDocument();
      const status = await screen.findByRole("status");

      await user.click(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      );

      expect(status).toHaveTextContent("Crafting MFU…");
      expect(ipc.generateStreamingNotes).toHaveBeenCalledWith(1);

      resolveCraft(openedSession({ windows: ONE_WINDOW, notes: NOTES }));

      expect(
        await screen.findByText("Discussed Q3 roadmap."),
      ).toBeInTheDocument();
      expect(screen.getByText("Ship M1 by Friday.")).toBeInTheDocument();
      expect(screen.queryByText("Run MFU Craft")).not.toBeInTheDocument();
      await waitFor(() => expect(status).toHaveTextContent("Ready"));
    });

    it("ticks an elapsed timer once per second while Crafting MFU…", async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      try {
        const user = userEvent.setup({
          advanceTimers: vi.advanceTimersByTime.bind(vi),
        });
        vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
        vi.mocked(ipc.openStreamingSession).mockResolvedValue(
          openedSession({ windows: ONE_WINDOW }),
        );
        vi.mocked(ipc.generateStreamingNotes).mockReturnValue(
          new Promise(() => {
            // Never resolves — only the ticking timer is under test here.
          }),
        );
        render(<StreamingView onClose={vi.fn()} />);
        await user.click(await screen.findByText("Standup"));
        const status = await screen.findByRole("status");

        await user.click(
          await screen.findByRole("button", { name: "Craft MFU notes" }),
        );
        expect(status.querySelector(".wp-status-timer")?.textContent).toBe(
          "00:00",
        );

        await vi.advanceTimersByTimeAsync(3000);

        expect(status.querySelector(".wp-status-timer")?.textContent).toBe(
          "00:03",
        );
      } finally {
        vi.useRealTimers();
      }
    });

    // Empty notes sections are omitted, matching Meeting's rendering
    it("omits empty notes sections", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingNotes).mockResolvedValue(
        openedSession({
          windows: ONE_WINDOW,
          notes: { ...NOTES, participants: "" },
        }),
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      );

      await screen.findByText("Discussed Q3 roadmap.");
      expect(screen.queryByText("Participants")).not.toBeInTheDocument();
    });

    // S-7: notes persist across reopen
    it("shows previously generated notes when reopening a session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW, notes: NOTES }),
      );
      render(<StreamingView onClose={vi.fn()} />);

      await user.click(await screen.findByText("Standup"));

      expect(
        await screen.findByText("Discussed Q3 roadmap."),
      ).toBeInTheDocument();
      expect(ipc.generateStreamingNotes).not.toHaveBeenCalled();
    });

    // S-2: failure surfaces in the error banner and the widget
    it("shows the error banner and MFU Failed on a Craft failure", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingNotes).mockRejectedValue(
        "no LLM model selected in Settings",
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");

      await user.click(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      );

      expect(
        await screen.findByText(/no LLM model selected in Settings/),
      ).toBeInTheDocument();
      await waitFor(() => expect(status).toHaveTextContent("MFU Failed"));
    });

    // MFU Failed persists until the next relevant action, not an auto-timeout
    it("keeps showing MFU Failed until the next Craft attempt", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingNotes).mockRejectedValueOnce("failed");
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      );
      await waitFor(() => expect(status).toHaveTextContent("MFU Failed"));

      // Time passing alone does not clear it — no auto-revert timer.
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(status).toHaveTextContent("MFU Failed");

      vi.mocked(ipc.generateStreamingNotes).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW, notes: NOTES }),
      );
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      );

      expect(status).not.toHaveTextContent("MFU Failed");
    });

    // Starting a new session also clears a stale MFU Failed
    it("clears MFU Failed when starting a new session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingNotes).mockRejectedValue("failed");
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 2,
        title: "New Streaming Session",
        created_at_ms: 200,
        updated_at_ms: 200,
        status: "active",
      });
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      );
      await waitFor(() => expect(status).toHaveTextContent("MFU Failed"));

      await user.click(await screen.findByRole("button", { name: "Start" }));

      expect(status).not.toHaveTextContent("MFU Failed");
    });

    // Change-hygiene §1: craftFailed is new state introduced by this task —
    // Delete is a removal path that must clear it too, not just Start/Open.
    it("clears MFU Failed when deleting the active session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingNotes).mockRejectedValue("failed");
      vi.mocked(ipc.deleteStreamingSession).mockResolvedValue(undefined);
      vi.spyOn(window, "confirm").mockReturnValue(true);
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      );
      await waitFor(() => expect(status).toHaveTextContent("MFU Failed"));

      await user.click(
        await screen.findByRole("button", { name: "Delete Standup" }),
      );

      expect(status).not.toHaveTextContent("MFU Failed");
    });

    // S-13: same-session double-trigger
    it("disables Craft while a request is already in flight", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      let resolveCraft!: (v: StreamingSession) => void;
      vi.mocked(ipc.generateStreamingNotes).mockReturnValue(
        new Promise((resolve) => {
          resolveCraft = resolve;
        }),
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const craftButton = await screen.findByRole("button", {
        name: "Craft MFU notes",
      });

      await user.click(craftButton);
      expect(craftButton).toBeDisabled();

      resolveCraft(openedSession({ windows: ONE_WINDOW, notes: NOTES }));
      await waitFor(() => expect(craftButton).not.toBeDisabled());
    });

    // S-10: switching sessions during an in-flight Craft
    it("does not attribute a stale Craft result to a newly opened session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([
        SESSION_A,
        { ...SESSION_A, id: 2, title: "Design Review" },
      ]);
      vi.mocked(ipc.openStreamingSession).mockImplementation(async (id) =>
        openedSession({
          id,
          title: id === 1 ? "Standup" : "Design Review",
          windows: ONE_WINDOW,
        }),
      );
      let resolveCraft!: (v: StreamingSession) => void;
      vi.mocked(ipc.generateStreamingNotes).mockReturnValue(
        new Promise((resolve) => {
          resolveCraft = resolve;
        }),
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      );

      await user.click(await screen.findByText("Design Review"));
      resolveCraft(
        openedSession({
          id: 1,
          title: "Standup",
          windows: ONE_WINDOW,
          notes: NOTES,
        }),
      );

      expect(
        screen.queryByText("Discussed Q3 roadmap."),
      ).not.toBeInTheDocument();
      expect(screen.getByText("Run MFU Craft")).toBeInTheDocument();
    });
  });

  describe("Prettify", () => {
    // S-1: happy path
    it("shows a diff with Accept/Cancel after Prettify, then applies the accepted text", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({
          windows: [
            {
              window_index: 0,
              start_ms: 0,
              end_ms: 7000,
              text: "so like hello there friend",
              language: "en",
              outcome_ok: true,
            },
          ],
        }),
      );
      let resolvePrettify!: (v: string) => void;
      vi.mocked(ipc.generateStreamingPrettify).mockReturnValue(
        new Promise((resolve) => {
          resolvePrettify = resolve;
        }),
      );
      vi.mocked(ipc.acceptStreamingPrettify).mockResolvedValue(
        openedSession({
          windows: [
            {
              window_index: 0,
              start_ms: 0,
              end_ms: 7000,
              text: "so like hello there friend",
              language: "en",
              outcome_ok: true,
            },
          ],
          prettified_text: "Hello there, friend.",
        }),
      );
      const { container } = render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");

      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      expect(status).toHaveTextContent("Prettifying…");
      expect(ipc.generateStreamingPrettify).toHaveBeenCalledWith(1);

      resolvePrettify("Hello there, friend.");
      const acceptButton = await screen.findByRole("button", {
        name: "Accept Prettify",
      });
      await screen.findByRole("button", { name: "Cancel Prettify" });
      expect(container.querySelector(".diff-del")).not.toBeNull();
      await waitFor(() => expect(status).toHaveTextContent("Ready"));

      await user.click(acceptButton);

      expect(ipc.acceptStreamingPrettify).toHaveBeenCalledWith(
        1,
        "Hello there, friend.",
      );
      await waitFor(() =>
        expect(
          screen.queryByRole("button", { name: "Accept Prettify" }),
        ).not.toBeInTheDocument(),
      );
      expect(
        await screen.findByText("Hello there, friend."),
      ).toBeInTheDocument();
    });

    // S-2: Cancel discards with no persistence
    it("Cancel discards the pending review with no backend call", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockResolvedValue(
        "hello there (cleaned)",
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      await screen.findByRole("button", { name: "Accept Prettify" });

      await user.click(
        await screen.findByRole("button", { name: "Cancel Prettify" }),
      );

      expect(ipc.acceptStreamingPrettify).not.toHaveBeenCalled();
      expect(
        screen.queryByRole("button", { name: "Accept Prettify" }),
      ).not.toBeInTheDocument();
      expect(screen.getByText(/hello there/)).toBeInTheDocument();
    });

    // S-3, EP: text/no-text partition
    it("disables Prettify when the session has only fail-open windows", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({
          windows: [
            {
              window_index: 0,
              start_ms: 0,
              end_ms: 7000,
              text: "",
              language: "auto",
              outcome_ok: false,
            },
          ],
        }),
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      expect(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      ).toBeDisabled();
    });

    // S-4, EP: running/stopped partition
    it("disables Prettify while the session is running", async () => {
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
      windowHandler!({ ...ONE_WINDOW[0], session_id: 2 });

      expect(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      ).toBeDisabled();
    });

    // S-5: mutual exclusion with Craft, both directions
    it("disables Prettify while Craft is in flight, and Craft while Prettify is in flight", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingNotes).mockReturnValue(
        new Promise(() => {}),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockReturnValue(
        new Promise(() => {}),
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      );
      expect(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      ).toBeDisabled();
    });

    // S-5, decision-table: the reverse direction of the guard above — a
    // Prettify request in flight also disables Craft.
    it("disables Craft while Prettify is in flight", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockReturnValue(
        new Promise(() => {}),
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );

      expect(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      ).toBeDisabled();
    });

    // S-6 + S-7: failure surfaces and persists
    it("shows the error banner and Prettify Failed on failure, persisting until the next action", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockRejectedValue(
        "no LLM model selected in Settings",
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");

      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );

      expect(
        await screen.findByText(/no LLM model selected in Settings/),
      ).toBeInTheDocument();
      await waitFor(() => expect(status).toHaveTextContent("Prettify Failed"));

      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(status).toHaveTextContent("Prettify Failed");
    });

    // S-7: cleared by the next Prettify attempt, not by time alone
    it("clears Prettify Failed on the next successful Prettify attempt", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockRejectedValueOnce("failed");
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      await waitFor(() => expect(status).toHaveTextContent("Prettify Failed"));

      vi.mocked(ipc.generateStreamingPrettify).mockResolvedValue(
        "cleaned text",
      );
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );

      expect(status).not.toHaveTextContent("Prettify Failed");
    });

    // S-7: cleared by starting a new session too
    it("clears Prettify Failed when starting a new session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockRejectedValue("failed");
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 2,
        title: "New Streaming Session",
        created_at_ms: 200,
        updated_at_ms: 200,
        status: "active",
      });
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      await waitFor(() => expect(status).toHaveTextContent("Prettify Failed"));

      await user.click(await screen.findByRole("button", { name: "Start" }));

      expect(status).not.toHaveTextContent("Prettify Failed");
    });

    // S-8: delete clears Prettify Failed
    it("clears Prettify Failed when deleting the active session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockRejectedValue("failed");
      vi.mocked(ipc.deleteStreamingSession).mockResolvedValue(undefined);
      vi.spyOn(window, "confirm").mockReturnValue(true);
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      await waitFor(() => expect(status).toHaveTextContent("Prettify Failed"));

      await user.click(
        await screen.findByRole("button", { name: "Delete Standup" }),
      );

      expect(status).not.toHaveTextContent("Prettify Failed");
    });

    // S-9, decision-table: Prettify-in-flight/not
    it("disables Prettify while a request is already in flight", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockReturnValue(
        new Promise(() => {}),
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const prettifyButton = await screen.findByRole("button", {
        name: "Prettify transcript",
      });

      await user.click(prettifyButton);

      expect(prettifyButton).toBeDisabled();
    });

    // S-10: cross-session isolation
    it("does not attribute a stale Prettify result to a newly opened session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([
        SESSION_A,
        { ...SESSION_A, id: 2, title: "Design Review" },
      ]);
      vi.mocked(ipc.openStreamingSession).mockImplementation(async (id) =>
        openedSession({
          id,
          title: id === 1 ? "Standup" : "Design Review",
          windows: ONE_WINDOW,
        }),
      );
      let resolvePrettify!: (v: string) => void;
      vi.mocked(ipc.generateStreamingPrettify).mockReturnValue(
        new Promise((resolve) => {
          resolvePrettify = resolve;
        }),
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );

      await user.click(await screen.findByText("Design Review"));
      resolvePrettify("cleaned text");

      expect(
        screen.queryByRole("button", { name: "Accept Prettify" }),
      ).not.toBeInTheDocument();
    });

    // S-19, decision-table: review-pending/not
    it("disables Prettify while a review is already pending", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockResolvedValue(
        "cleaned text",
      );
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      await screen.findByRole("button", { name: "Accept Prettify" });

      expect(
        screen.getByRole("button", { name: "Prettify transcript" }),
      ).toBeDisabled();
    });

    // S-20: delete clears a pending review
    it("clears a pending review when deleting the active session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockResolvedValue(
        "cleaned text",
      );
      vi.mocked(ipc.deleteStreamingSession).mockResolvedValue(undefined);
      vi.spyOn(window, "confirm").mockReturnValue(true);
      render(<StreamingView onClose={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      await screen.findByRole("button", { name: "Accept Prettify" });

      await user.click(
        await screen.findByRole("button", { name: "Delete Standup" }),
      );

      expect(
        screen.queryByRole("button", { name: "Accept Prettify" }),
      ).not.toBeInTheDocument();
    });

    // Accepted text persists across reopen and is used by Copy/Export
    it("shows a previously accepted prettified text on reopen and uses it for Copy", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({
          windows: ONE_WINDOW,
          prettified_text: "Already accepted clean text.",
        }),
      );
      render(<StreamingView onClose={vi.fn()} />);

      await user.click(await screen.findByText("Standup"));

      expect(
        await screen.findByText("Already accepted clean text."),
      ).toBeInTheDocument();

      await user.click(
        await screen.findByRole("button", { name: "Copy transcript" }),
      );
      expect(writeTextMock).toHaveBeenCalledWith(
        "Already accepted clean text.",
      );
    });
  });
});
