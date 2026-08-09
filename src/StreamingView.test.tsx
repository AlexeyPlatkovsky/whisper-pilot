import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
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
const revertPrettifyMock = vi.hoisted(() => vi.fn());

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
  revertStreamingPrettify: revertPrettifyMock,
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
  revertPrettifyMock.mockReset();
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

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    expect(await screen.findByText("Standup")).toBeInTheDocument();
  });

  it("filters sessions by title only after three characters and shows no matches", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([
      SESSION_A,
      { ...SESSION_A, id: 2, title: "Retro" },
    ]);

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    const search = await screen.findByLabelText("Search sessions");
    // BVA: filtering starts at exactly three characters and resets at two.
    await user.type(search, "sta");
    expect(screen.getByText("Standup")).toBeInTheDocument();
    expect(screen.queryByText("Retro")).not.toBeInTheDocument();

    await user.type(search, "x");
    expect(await screen.findByText("No matches")).toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "st");
    expect(screen.getByText("Standup")).toBeInTheDocument();
    expect(screen.getByText("Retro")).toBeInTheDocument();
  });

  it("shows the empty state before any session is started or opened", async () => {
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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

  it("keeps windows ordered when they arrive out of order", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());

    windowHandler!({
      ...ONE_WINDOW[0],
      session_id: 2,
      window_index: 1,
      text: "second",
    });
    expect(await screen.findByText(/second/)).toBeInTheDocument();
    windowHandler!({
      ...ONE_WINDOW[0],
      session_id: 2,
      window_index: 0,
      text: "first",
    });

    const transcript = await screen.findByText(/first/);
    expect(transcript.parentElement?.textContent).toMatch(/first.*second/);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));

    expect(await screen.findByText(/past meeting notes/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
  });

  describe("resuming a stopped session", () => {
    // S-9: opening a past stopped session relabels Start to Resume
    it("relabels Start to Resume once a past stopped session is open", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      expect(
        await screen.findByRole("button", { name: "Start" }),
      ).toBeInTheDocument();

      await user.click(await screen.findByText("Standup"));

      expect(
        await screen.findByRole("button", { name: "Resume" }),
      ).toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "Start" }),
      ).not.toBeInTheDocument();
    });

    // S-10: happy path — Resume passes the open session's id and keeps its
    // existing windows rather than clearing them like a fresh start does.
    it("Resume continues the open session's transcript instead of replacing it", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 1,
        title: "Standup",
        created_at_ms: 100,
        updated_at_ms: 500,
        status: "active",
      });
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      expect(await screen.findByText(/hello there/)).toBeInTheDocument();

      await user.click(await screen.findByRole("button", { name: "Resume" }));

      expect(ipc.startStreamingSession).toHaveBeenCalledWith(1);
      // Still showing the previously-loaded window, not cleared to empty.
      expect(screen.getByText(/hello there/)).toBeInTheDocument();
      expect(
        await screen.findByRole("button", { name: "Stop" }),
      ).not.toBeDisabled();
    });

    // S-11: a resumed session keeps appending onto its prior windows, using
    // the continued window_index the backend hands back via the event.
    it("a window appended after Resume joins the existing transcript, not replaces it", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 1,
        title: "Standup",
        created_at_ms: 100,
        updated_at_ms: 500,
        status: "active",
      });
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      await user.click(await screen.findByRole("button", { name: "Resume" }));
      await waitFor(() => expect(windowHandler).not.toBeNull());

      windowHandler!({
        session_id: 1,
        window_index: 1,
        start_ms: 7000,
        end_ms: 14000,
        text: "continued",
        language: "en",
        outcome_ok: true,
      });

      expect(await screen.findByText(/continued/)).toBeInTheDocument();
      expect(screen.getByText(/hello there/)).toBeInTheDocument();
    });

    // S-12: the "+" (New) icon always starts fresh, even with a stopped
    // session open — it must not be redirected into a resume.
    it("the New icon always starts a brand-new session, not a resume", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 2,
        title: "New Streaming Session",
        created_at_ms: 200,
        updated_at_ms: 200,
        status: "active",
      });
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      expect(await screen.findByText(/hello there/)).toBeInTheDocument();

      await user.click(
        screen.getByRole("button", { name: "New streaming session" }),
      );

      expect(ipc.startStreamingSession).toHaveBeenCalledWith(undefined);
      expect(screen.queryByText(/hello there/)).not.toBeInTheDocument();
      expect(await screen.findByText("Listening…")).toBeInTheDocument();
    });

    // S-13: a failed resume surfaces the error without discarding the
    // session's existing windows.
    it("a failed resume surfaces the error and keeps the existing transcript visible", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.startStreamingSession).mockRejectedValue(
        "only a stopped Streaming session can be resumed",
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(await screen.findByRole("button", { name: "Resume" }));

      expect(
        await screen.findByText(/only a stopped Streaming session/),
      ).toBeInTheDocument();
      expect(screen.getByText(/hello there/)).toBeInTheDocument();
    });
  });

  describe("rename/delete dialogs", () => {
    // These use in-app modals, not window.prompt/window.confirm — Tauri's
    // WKWebView doesn't reliably wire up the native JS dialog delegate, so
    // those silently no-op instead of showing anything (the bug this covers).
    it("renaming a session from its sidebar row calls the IPC command with the new title", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.renameStreamingSession).mockResolvedValue(
        openedSession({ title: "Renamed" }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await screen.findByText("Standup");

      await user.click(screen.getByRole("button", { name: "Rename Standup" }));
      const dialog = screen.getByRole("dialog", { name: "Rename session" });
      const input = within(dialog).getByRole("textbox", {
        name: "Session label",
      });
      await user.clear(input);
      await user.type(input, "Renamed");
      await user.click(within(dialog).getByRole("button", { name: "Save" }));

      expect(ipc.renameStreamingSession).toHaveBeenCalledWith(1, "Renamed");
      expect(
        screen.queryByRole("dialog", { name: "Rename session" }),
      ).not.toBeInTheDocument();
    });

    it("renaming the open session from the header title also renames it", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.renameStreamingSession).mockResolvedValue(
        openedSession({ title: "Renamed" }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(await screen.findByText("Standup"));
      await user.click(screen.getByRole("button", { name: "Rename session" }));
      const dialog = screen.getByRole("dialog", { name: "Rename session" });
      const input = within(dialog).getByRole("textbox", {
        name: "Session label",
      });
      await user.clear(input);
      await user.type(input, "Renamed");
      await user.click(within(dialog).getByRole("button", { name: "Save" }));

      expect(ipc.renameStreamingSession).toHaveBeenCalledWith(1, "Renamed");
      expect(
        await screen.findByRole("heading", { name: "Renamed" }),
      ).toBeInTheDocument();

      await user.click(
        screen.getByRole("button", { name: "Export as Markdown" }),
      );
      expect(ipc.saveTextDialog).toHaveBeenCalledWith(
        expect.stringContaining("# Renamed"),
        "Renamed.md",
      );
    });

    // [EP + BVA]
    it("rejects blank and 121-character titles without writing", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await screen.findByText("Standup");

      await user.click(screen.getByRole("button", { name: "Rename Standup" }));
      const dialog = screen.getByRole("dialog", { name: "Rename session" });
      const input = within(dialog).getByRole("textbox", {
        name: "Session label",
      });
      await user.clear(input);
      await user.type(input, "   ");
      await user.click(within(dialog).getByRole("button", { name: "Save" }));
      expect(within(dialog).getByRole("alert")).toHaveTextContent(
        "Session label is required",
      );

      await user.clear(input);
      await user.type(input, "a".repeat(121));
      await user.click(within(dialog).getByRole("button", { name: "Save" }));
      expect(within(dialog).getByRole("alert")).toHaveTextContent(
        "Session label must be 120 characters or fewer",
      );
      expect(ipc.renameStreamingSession).not.toHaveBeenCalled();
    });

    it("Escape closes the rename dialog without saving", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await screen.findByText("Standup");

      await user.click(screen.getByRole("button", { name: "Rename Standup" }));
      expect(
        screen.getByRole("dialog", { name: "Rename session" }),
      ).toBeInTheDocument();

      await user.keyboard("{Escape}");

      expect(
        screen.queryByRole("dialog", { name: "Rename session" }),
      ).not.toBeInTheDocument();
      expect(ipc.renameStreamingSession).not.toHaveBeenCalled();
    });

    it("deleting a session asks for confirmation and calls the IPC command", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.deleteStreamingSession).mockResolvedValue(undefined);
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await screen.findByText("Standup");

      await user.click(screen.getByRole("button", { name: "Delete Standup" }));
      const dialog = screen.getByRole("alertdialog", {
        name: "Delete Standup",
      });
      await user.click(within(dialog).getByRole("button", { name: "Delete" }));

      expect(ipc.deleteStreamingSession).toHaveBeenCalledWith(1);
      expect(
        screen.queryByRole("alertdialog", { name: "Delete Standup" }),
      ).not.toBeInTheDocument();
    });

    it("Cancel on the delete dialog makes no IPC call", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await screen.findByText("Standup");

      await user.click(screen.getByRole("button", { name: "Delete Standup" }));
      const dialog = screen.getByRole("alertdialog", {
        name: "Delete Standup",
      });
      await user.click(within(dialog).getByRole("button", { name: "Cancel" }));

      expect(ipc.deleteStreamingSession).not.toHaveBeenCalled();
      expect(
        screen.queryByRole("alertdialog", { name: "Delete Standup" }),
      ).not.toBeInTheDocument();
    });

    it("also opens rename/delete from the header title's icons for the open session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(screen.getByRole("button", { name: "Delete session" }));

      expect(
        screen.getByRole("alertdialog", { name: "Delete Standup" }),
      ).toBeInTheDocument();
    });
  });

  it("calls onClose when the sidebar Meeting toggle is clicked", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<StreamingView onClose={onClose} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByRole("button", { name: "Meeting" }));

    expect(onClose).toHaveBeenCalled();
  });

  it("unlistens all three event handlers on unmount", async () => {
    const { unmount } = render(
      <StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />,
    );
    await waitFor(() => expect(windowHandler).not.toBeNull());

    unmount();

    expect(windowHandler).toBeNull();
    expect(sourcesHandler).toBeNull();
    expect(endedHandler).toBeNull();
  });

  it("unmounting before event registration resolves still unlistens once it does (no leaked listeners)", async () => {
    const { unmount } = render(
      <StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />,
    );

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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));

    expect(
      await screen.findByText(/streaming session 1 was not found/),
    ).toBeInTheDocument();
  });

  it("a failed rename surfaces the error", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.renameStreamingSession).mockRejectedValue("rename failed");
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await screen.findByText("Standup");

    await user.click(screen.getByRole("button", { name: "Rename Standup" }));
    const dialog = screen.getByRole("dialog", { name: "Rename session" });
    const input = within(dialog).getByRole("textbox", {
      name: "Session label",
    });
    await user.clear(input);
    await user.type(input, "Renamed");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(await screen.findByText(/rename failed/)).toBeInTheDocument();
  });

  it("a failed delete surfaces the error", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.deleteStreamingSession).mockRejectedValue("delete failed");
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await screen.findByText("Standup");

    await user.click(screen.getByRole("button", { name: "Delete Standup" }));
    const dialog = screen.getByRole("alertdialog", { name: "Delete Standup" });
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));

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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));
    await screen.findByText(/notes to be cleared/);
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([]);

    await user.click(screen.getByRole("button", { name: "Delete Standup" }));
    const dialog = screen.getByRole("alertdialog", { name: "Delete Standup" });
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));

    expect(screen.queryByText(/notes to be cleared/)).not.toBeInTheDocument();
    expect(
      await screen.findByText(/Start a session, or open one/),
    ).toBeInTheDocument();
  });

  it("Copy and Export are disabled until there is transcript text", async () => {
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));

    await user.click(
      await screen.findByRole("button", { name: "Copy transcript" }),
    );

    expect(
      await screen.findByRole("button", { name: "Copied" }),
    ).toBeInTheDocument();
    expect(writeTextMock).toHaveBeenCalledWith("hello [unavailable]");
  });

  // state-transition: idle → copied → idle (timeout rollback).
  it("shows a 'Copied' toast and a checked button after a successful copy, then rolls back", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = userEvent.setup({
        advanceTimers: vi.advanceTimersByTime.bind(vi),
      });
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(
        await screen.findByRole("button", { name: "Copy transcript" }),
      );

      const toast = await screen.findByText("Copied", {
        selector: ".wp-toast",
      });
      expect(toast).toHaveAttribute("role", "status");
      expect(
        screen.getByRole("button", { name: "Copied" }),
      ).toBeInTheDocument();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2600);
      });

      expect(
        screen.queryByText("Copied", { selector: ".wp-toast" }),
      ).not.toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Copy transcript" }),
      ).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  // state-transition: copied --re-click--> copied (the timeout restarts).
  it("restarts the feedback timeout when Copy is clicked again before the rollback", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = userEvent.setup({
        advanceTimers: vi.advanceTimersByTime.bind(vi),
      });
      // Installed after userEvent.setup, which swaps navigator.clipboard for
      // its own stub — defining ours last is what the component actually calls.
      const writeText = vi.fn().mockResolvedValue(undefined);
      Object.defineProperty(navigator, "clipboard", {
        value: { writeText },
        configurable: true,
      });
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(
        await screen.findByRole("button", { name: "Copy transcript" }),
      );
      await screen.findByText("Copied", { selector: ".wp-toast" });

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      await user.click(screen.getByRole("button", { name: "Copied" }));
      expect(writeText).toHaveBeenCalledTimes(2);

      // 2s after the second click the feedback must still be visible…
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(
        screen.getByText("Copied", { selector: ".wp-toast" }),
      ).toBeInTheDocument();

      // …and only the restarted timeout (2.5s) rolls it back.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(600);
      });
      expect(
        screen.queryByText("Copied", { selector: ".wp-toast" }),
      ).not.toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Copy transcript" }),
      ).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  // state-transition: copied --switch session--> idle (pending feedback cleared).
  it("clears a pending Copied feedback when another session is opened", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = userEvent.setup({
        advanceTimers: vi.advanceTimersByTime.bind(vi),
      });
      const SESSION_B: StreamingSessionSummary = {
        id: 2,
        title: "Retro",
        created_at_ms: 200,
        updated_at_ms: 200,
        status: "stopped",
      };
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([
        SESSION_A,
        SESSION_B,
      ]);
      vi.mocked(ipc.openStreamingSession).mockImplementation(async (id) =>
        id === SESSION_B.id
          ? openedSession({ id: 2, title: "Retro", windows: ONE_WINDOW })
          : openedSession({ windows: ONE_WINDOW }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(
        await screen.findByRole("button", { name: "Copy transcript" }),
      );
      await screen.findByText("Copied", { selector: ".wp-toast" });

      await user.click(screen.getByText("Retro"));
      await screen.findByRole("heading", { name: "Retro" });

      expect(
        screen.queryByText("Copied", { selector: ".wp-toast" }),
      ).not.toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Copy transcript" }),
      ).toBeInTheDocument();

      // The cancelled timer must not resurrect the feedback on the new session.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(
        screen.queryByText("Copied", { selector: ".wp-toast" }),
      ).not.toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Copy transcript" }),
      ).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  // state-transition: copying (in-flight write) --switch session--> the late
  // resolution must not enter the copied state on the new session.
  it("does not paint Copied feedback onto a session opened while the clipboard write is in flight", async () => {
    const user = userEvent.setup();
    // Installed after userEvent.setup, which swaps navigator.clipboard for
    // its own stub — defining ours last is what the component actually calls.
    let resolveWrite: () => void = () => {};
    const writeText = vi.fn().mockReturnValue(
      new Promise<void>((resolve) => {
        resolveWrite = resolve;
      }),
    );
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    const SESSION_B: StreamingSessionSummary = {
      id: 2,
      title: "Retro",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "stopped",
    };
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([
      SESSION_A,
      SESSION_B,
    ]);
    vi.mocked(ipc.openStreamingSession).mockImplementation(async (id) =>
      id === SESSION_B.id
        ? openedSession({ id: 2, title: "Retro", windows: ONE_WINDOW })
        : openedSession({ windows: ONE_WINDOW }),
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));

    await user.click(
      await screen.findByRole("button", { name: "Copy transcript" }),
    );
    // The write is still in flight: no feedback yet.
    expect(
      screen.queryByText("Copied", { selector: ".wp-toast" }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByText("Retro"));
    await screen.findByRole("heading", { name: "Retro" });

    // The write for the previous session resolves only now.
    resolveWrite();
    await act(async () => {});

    expect(
      screen.queryByText("Copied", { selector: ".wp-toast" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Copy transcript" }),
    ).toBeInTheDocument();
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));

    await user.click(
      await screen.findByRole("button", { name: "Copy transcript" }),
    );

    expect(await screen.findByText(/denied/)).toBeInTheDocument();
    expect(
      screen.queryByText("Copied", { selector: ".wp-toast" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Copy transcript" }),
    ).toBeInTheDocument();
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
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));

    await user.click(
      await screen.findByRole("button", { name: "Export as Markdown" }),
    );

    expect(await screen.findByText(/disk full/)).toBeInTheDocument();
  });

  describe("status widget", () => {
    // S-2: first-launch / empty state
    it("shows Ready with no timer before any session has run", async () => {
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
        render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
        const status = await screen.findByRole("status");

        await user.click(await screen.findByRole("button", { name: "Start" }));
        await waitFor(() => expect(status).toHaveTextContent("On Air"));

        // The advance fires one interval callback per simulated second, and
        // firing thousands of them takes real time — seconds under coverage
        // instrumentation — which shouldAdvanceTime bleeds into the measured
        // elapsed. Assert bleed-tolerant windows on each side of the 60
        // minute format switch instead of exact boundary seconds.
        await vi.advanceTimersByTimeAsync(59 * 60 * 1000 + 30 * 1000);
        expect(status.querySelector(".wp-status-timer")?.textContent).toMatch(
          /^59:\d{2}$/,
        );

        await vi.advanceTimersByTimeAsync(60 * 1000);
        expect(status.querySelector(".wp-status-timer")?.textContent).toMatch(
          /^1:00:\d{2}$/,
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      expect(
        screen.getByRole("button", { name: "Craft MFU notes" }),
      ).toBeDisabled();

      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      // A second click re-opens the same session; by now its title is also
      // shown in the header, so target the sidebar row specifically rather
      // than the now-ambiguous "Standup" text.
      await user.click(
        await screen.findByRole("button", { name: "Open Standup" }),
      );

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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
        render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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

    // Starting again (here: resuming the open session) also clears a stale
    // MFU Failed — the id matches SESSION_A's, as a real resume returns.
    it("clears MFU Failed when starting again", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingNotes).mockRejectedValue("failed");
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 1,
        title: "Standup",
        created_at_ms: 100,
        updated_at_ms: 200,
        status: "active",
      });
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      );
      await waitFor(() => expect(status).toHaveTextContent("MFU Failed"));

      await user.click(await screen.findByRole("button", { name: "Resume" }));

      expect(ipc.startStreamingSession).toHaveBeenCalledWith(1);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU notes" }),
      );
      await waitFor(() => expect(status).toHaveTextContent("MFU Failed"));

      await user.click(
        await screen.findByRole("button", { name: "Delete Standup" }),
      );
      await user.click(
        within(
          screen.getByRole("alertdialog", { name: "Delete Standup" }),
        ).getByRole("button", { name: "Delete" }),
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      const { container } = render(
        <StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />,
      );
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

    // S-1 failure path: an Accept persistence error leaves the review visible.
    it("shows an error when accepting a prettification fails", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockResolvedValue(
        "hello there (cleaned)",
      );
      vi.mocked(ipc.acceptStreamingPrettify).mockRejectedValue(
        "could not save prettified transcript",
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      await user.click(
        await screen.findByRole("button", { name: "Accept Prettify" }),
      );

      expect(
        await screen.findByText("could not save prettified transcript"),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Cancel Prettify" }),
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
    // Starting again (here: resuming the open session) also clears a stale
    // Prettify Failed — the id matches SESSION_A's, as a real resume returns.
    it("clears Prettify Failed when starting again", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockRejectedValue("failed");
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 1,
        title: "Standup",
        created_at_ms: 100,
        updated_at_ms: 200,
        status: "active",
      });
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      await waitFor(() => expect(status).toHaveTextContent("Prettify Failed"));

      await user.click(await screen.findByRole("button", { name: "Resume" }));

      expect(ipc.startStreamingSession).toHaveBeenCalledWith(1);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      await waitFor(() => expect(status).toHaveTextContent("Prettify Failed"));

      await user.click(
        await screen.findByRole("button", { name: "Delete Standup" }),
      );
      await user.click(
        within(
          screen.getByRole("alertdialog", { name: "Delete Standup" }),
        ).getByRole("button", { name: "Delete" }),
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      await screen.findByRole("button", { name: "Accept Prettify" });

      await user.click(
        await screen.findByRole("button", { name: "Delete Standup" }),
      );
      await user.click(
        within(
          screen.getByRole("alertdialog", { name: "Delete Standup" }),
        ).getByRole("button", { name: "Delete" }),
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
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

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

    // S-21: accepted-state Cancel/Revert restores the raw transcript.
    it("reverts an accepted prettification back to the raw transcript", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({
          windows: ONE_WINDOW,
          prettified_text: "Already accepted clean text.",
        }),
      );
      revertPrettifyMock.mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(await screen.findByText("Standup"));
      expect(
        await screen.findByText("Already accepted clean text."),
      ).toBeInTheDocument();

      await user.click(
        await screen.findByRole("button", { name: "Cancel Prettify" }),
      );

      expect(revertPrettifyMock).toHaveBeenCalledWith(1);
      expect(await screen.findByText(/hello there/)).toBeInTheDocument();
      expect(
        screen.queryByText("Already accepted clean text."),
      ).not.toBeInTheDocument();
    });

    // S-22: revert failure remains visible and does not discard the accepted text.
    it("shows a revert error and keeps accepted text when the backend rejects", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({
          windows: ONE_WINDOW,
          prettified_text: "Already accepted clean text.",
        }),
      );
      revertPrettifyMock.mockRejectedValue("could not revert transcript");
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Cancel Prettify" }),
      );

      expect(
        await screen.findByText("could not revert transcript"),
      ).toBeInTheDocument();
      expect(
        screen.getByText("Already accepted clean text."),
      ).toBeInTheDocument();
    });
  });
});
