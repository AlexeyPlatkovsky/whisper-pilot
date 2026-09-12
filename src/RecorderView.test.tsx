import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ModeToggle } from "./ModeToggle";
import { RecorderView } from "./RecorderView";
import * as ipc from "./ipc";

type Handler<T> = (payload: T) => void;
type RecorderStatus =
  "recording" | "finalizing" | "completed" | "recoverable" | "delete_failed";

interface RecorderSegment {
  id: number;
  start_sample: number;
  end_sample: number;
  text: string;
  language: string;
}

interface RecorderSession {
  id: number;
  title: string;
  created_at_ms: number;
  duration_ms: number;
  status: RecorderStatus;
  sample_rate: number;
  segments: RecorderSegment[];
  recovery_reason?: string;
  polished_text?: string;
}

let liveHandler: Handler<ipc.LiveCaptureSnapshot> | null = null;
let sessionHandler: Handler<RecorderSession> | null = null;
let committedHandler: Handler<RecorderSegment & { session_id: number }> | null =
  null;
let partialHandler: Handler<{
  session_id: number;
  revision: number;
  text: string;
}> | null = null;

vi.mock("./ipc", () => ({
  listRecorderSessions: vi.fn(async () => []),
  openRecorderSession: vi.fn(),
  renameRecorderSession: vi.fn(),
  deleteRecorderSession: vi.fn(),
  startRecorderSession: vi.fn(),
  stopRecorderSession: vi.fn(),
  recoverRecorderSession: vi.fn(),
  updateRecorderSegment: vi.fn(),
  exportRecorderWav: vi.fn(),
  saveTextDialog: vi.fn(async () => null),
  generateRecorderPolish: vi.fn(),
  acceptRecorderPolish: vi.fn(),
  revertRecorderPolish: vi.fn(),
  getMicrophonePermissionStatus: vi.fn(async () => "authorized"),
  requestMicrophonePermission: vi.fn(),
  getLiveCaptureSnapshot: vi.fn(async () => ({
    phase: "idle",
    session_id: null,
    source: null,
    generation: 0,
    revision: 0,
    error: null,
  })),
  onLiveCaptureState: vi.fn(async (handler: Handler<unknown>) => {
    liveHandler = handler as Handler<ipc.LiveCaptureSnapshot>;
    return () => {
      liveHandler = null;
    };
  }),
  onRecorderSessionChanged: vi.fn(async (handler: Handler<unknown>) => {
    sessionHandler = handler as Handler<RecorderSession>;
    return () => {
      sessionHandler = null;
    };
  }),
  onRecorderSegmentCommitted: vi.fn(async (handler: Handler<unknown>) => {
    committedHandler = handler as Handler<
      RecorderSegment & { session_id: number }
    >;
    return () => {
      committedHandler = null;
    };
  }),
  onRecorderPartial: vi.fn(async (handler: Handler<unknown>) => {
    partialHandler = handler as typeof partialHandler;
    return () => {
      partialHandler = null;
    };
  }),
  onRecorderError: vi.fn(async () => () => {}),
}));

const FIRST_SEGMENT: RecorderSegment = {
  id: 101,
  start_sample: 0,
  end_sample: 24_000,
  text: "Committed phrase",
  language: "en",
};

const SESSION: RecorderSession = {
  id: 1,
  title: "Voice note",
  created_at_ms: 100,
  duration_ms: 1_000,
  status: "completed",
  sample_rate: 24_000,
  segments: [FIRST_SEGMENT],
};

const SECOND_SESSION: RecorderSession = {
  ...SESSION,
  id: 2,
  title: "Second note",
  segments: [{ ...FIRST_SEGMENT, id: 202, text: "Second phrase" }],
};

function renderRecorder(meetingTranscriptionActive = false) {
  return render(
    <RecorderView
      onSelectMeeting={vi.fn()}
      onSelectStreaming={vi.fn()}
      onOpenSettings={vi.fn()}
      meetingTranscriptionActive={meetingTranscriptionActive}
    />,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  liveHandler = null;
  sessionHandler = null;
  committedHandler = null;
  partialHandler = null;
  vi.mocked(ipc.listRecorderSessions).mockResolvedValue([SESSION]);
  vi.mocked(ipc.openRecorderSession).mockResolvedValue(SESSION);
  vi.mocked(ipc.renameRecorderSession).mockImplementation(
    async (_id, title) => ({
      ...SESSION,
      title,
    }),
  );
  vi.mocked(ipc.getMicrophonePermissionStatus).mockResolvedValue("authorized");
  vi.mocked(ipc.getLiveCaptureSnapshot).mockResolvedValue({
    phase: "idle",
    session_id: null,
    source: null,
    generation: 0,
    revision: 0,
    error: null,
  });
  vi.mocked(ipc.startRecorderSession).mockResolvedValue({
    ...SESSION,
    status: "recording",
  });
  vi.mocked(ipc.updateRecorderSegment).mockImplementation(
    async (_sessionId, _segmentId, text) => ({ ...FIRST_SEGMENT, text }),
  );
  vi.mocked(ipc.generateRecorderPolish).mockResolvedValue(
    "Polished committed phrase.",
  );
  vi.mocked(ipc.acceptRecorderPolish).mockResolvedValue({
    ...SESSION,
    polished_text: "Polished committed phrase.",
  });
  vi.mocked(ipc.revertRecorderPolish).mockResolvedValue(SESSION);
  if (!navigator.clipboard) {
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: async () => {} },
      configurable: true,
    });
  }
});

describe("Recorder mode", () => {
  it("is the third ModeToggle destination", async () => {
    const selectRecorder = vi.fn();
    const user = userEvent.setup();
    render(
      <ModeToggle
        mode="recorder"
        onSelectMeeting={vi.fn()}
        onSelectStreaming={vi.fn()}
        onSelectRecorder={selectRecorder}
      />,
    );

    expect(screen.getAllByRole("button")).toHaveLength(3);
    expect(screen.getByRole("button", { name: "Recorder" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(screen.getByRole("button", { name: "Recorder" }));
    expect(selectRecorder).toHaveBeenCalledOnce();
  });

  it("lists, opens, renames and deletes Recorder sessions", async () => {
    const user = userEvent.setup();
    renderRecorder();

    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );
    expect(ipc.openRecorderSession).toHaveBeenCalledWith(1);
    expect(
      await screen.findByDisplayValue("Committed phrase"),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Rename Voice note" }));
    const title = screen.getByRole("textbox", { name: "Recorder title" });
    await user.clear(title);
    await user.type(title, "Renamed note");
    await user.click(screen.getByRole("button", { name: "Save rename" }));
    expect(ipc.renameRecorderSession).toHaveBeenCalledWith(1, "Renamed note");

    await user.click(
      screen.getByRole("button", { name: "Delete Renamed note" }),
    );
    await user.click(screen.getByRole("button", { name: "Confirm delete" }));
    expect(ipc.deleteRecorderSession).toHaveBeenCalledWith(1);
  });

  it("fails closed until the backend snapshot arrives and blocks other owners", async () => {
    let resolveSnapshot!: (snapshot: ipc.LiveCaptureSnapshot) => void;
    vi.mocked(ipc.getLiveCaptureSnapshot).mockReturnValue(
      new Promise((resolve) => {
        resolveSnapshot = resolve;
      }),
    );
    const view = renderRecorder();

    expect(await screen.findByRole("button", { name: "Start" })).toBeDisabled();
    resolveSnapshot({
      phase: "idle",
      session_id: null,
      source: null,
      generation: 0,
      revision: 1,
      error: null,
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Start" })).toBeEnabled(),
    );

    act(() => {
      liveHandler?.({
        phase: "capturing",
        session_id: 7,
        source: "streaming",
        generation: 1,
        revision: 2,
        error: null,
      });
    });
    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();

    view.rerender(
      <RecorderView
        onSelectMeeting={vi.fn()}
        onSelectStreaming={vi.fn()}
        onOpenSettings={vi.fn()}
        meetingTranscriptionActive
      />,
    );
    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();

    view.unmount();
    vi.mocked(ipc.getLiveCaptureSnapshot).mockResolvedValue({
      phase: "idle",
      session_id: null,
      source: null,
      generation: 1,
      revision: 3,
      error: null,
    });
    vi.mocked(ipc.getMicrophonePermissionStatus).mockResolvedValue(
      "not_determined",
    );
    vi.mocked(ipc.requestMicrophonePermission).mockResolvedValue("denied");
    const user = userEvent.setup();
    renderRecorder();

    const start = await screen.findByRole("button", { name: "Start" });
    await waitFor(() => expect(start).toBeEnabled());
    expect(ipc.requestMicrophonePermission).not.toHaveBeenCalled();
    await user.click(start);

    expect(ipc.requestMicrophonePermission).toHaveBeenCalledOnce();
    expect(ipc.startRecorderSession).not.toHaveBeenCalled();
    expect(
      await screen.findByText(/microphone permission.*denied/i),
    ).toBeInTheDocument();
  });

  it("keeps committed rows stable while replacing one italic partial", async () => {
    const user = userEvent.setup();
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );
    await waitFor(() => expect(partialHandler).not.toBeNull());

    act(() => {
      sessionHandler?.({ ...SESSION, status: "recording" });
      partialHandler?.({ session_id: 99, revision: 500, text: "other note" });
      partialHandler?.({ session_id: 1, revision: 1, text: "first partial" });
      partialHandler?.({ session_id: 1, revision: 2, text: "new partial" });
      partialHandler?.({ session_id: 1, revision: 1, text: "stale partial" });
    });
    expect(screen.getByDisplayValue("Committed phrase")).toBeInTheDocument();
    expect(screen.getByText("new partial").closest("em")).not.toBeNull();
    expect(screen.queryByText("stale partial")).not.toBeInTheDocument();
    expect(screen.queryByText("other note")).not.toBeInTheDocument();

    act(() => {
      committedHandler?.({
        session_id: 1,
        id: 102,
        start_sample: 24_000,
        end_sample: 48_000,
        text: "Committed second phrase",
        language: "ru",
      });
    });
    expect(screen.getByDisplayValue("Committed phrase")).toBeInTheDocument();
    expect(
      screen.getByDisplayValue("Committed second phrase"),
    ).toBeInTheDocument();
    expect(screen.queryByText("new partial")).not.toBeInTheDocument();
  });

  it("renders authoritative lifecycle states and restores active capture on remount", async () => {
    const user = userEvent.setup();
    const { unmount } = renderRecorder();
    await waitFor(() => expect(sessionHandler).not.toBeNull());
    const start = await screen.findByRole("button", { name: "Start" });
    await waitFor(() => expect(start).toBeEnabled());
    await user.click(start);
    expect(ipc.startRecorderSession).toHaveBeenCalledOnce();
    for (const [status, label] of [
      ["recording", "Recording"],
      ["finalizing", "Finalizing"],
      ["completed", "Completed"],
      ["recoverable", "Recoverable"],
      ["delete_failed", "Delete failed"],
    ] as const) {
      act(() => sessionHandler?.({ ...SESSION, status }));
      expect(screen.getByRole("status")).toHaveTextContent(label);
      if (status === "recoverable") {
        expect(
          screen.getByRole("button", { name: "Recover" }),
        ).toBeInTheDocument();
      }
    }
    expect(
      screen.getByRole("button", { name: "Retry delete" }),
    ).toBeInTheDocument();
    act(() => {
      liveHandler?.({
        phase: "stopping",
        session_id: 1,
        source: "recorder",
        generation: 2,
        revision: 10,
        error: null,
      });
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "Stopping · Finalizing",
    );
    act(() => {
      liveHandler?.({
        phase: "error",
        session_id: 1,
        source: "recorder",
        generation: 2,
        revision: 11,
        error: "Microphone disconnected",
      });
    });
    expect(screen.getByRole("status")).toHaveTextContent("Error");
    expect(screen.getByRole("status")).toHaveClass("recorder-status--error");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Microphone disconnected",
    );

    unmount();
    vi.mocked(ipc.openRecorderSession).mockResolvedValue({
      ...SESSION,
      status: "recording",
      segments: [{ ...FIRST_SEGMENT, text: "Restored committed phrase" }],
    });
    vi.mocked(ipc.getLiveCaptureSnapshot).mockResolvedValue({
      phase: "capturing",
      session_id: 1,
      source: "recorder",
      generation: 4,
      revision: 9,
      error: null,
    });
    renderRecorder();
    const stop = await screen.findByRole("button", { name: "Stop" });
    expect(stop).toBeEnabled();
    await waitFor(() =>
      expect(ipc.openRecorderSession).toHaveBeenCalledWith(1),
    );
    expect(
      await screen.findByDisplayValue("Restored committed phrase"),
    ).toBeInTheDocument();
    act(() => {
      partialHandler?.({
        session_id: 1,
        revision: 1,
        text: "Continued after remount",
      });
    });
    expect(screen.getByText("Continued after remount")).toBeInTheDocument();
    await user.click(stop);
    expect(ipc.stopRecorderSession).toHaveBeenCalledOnce();
  });

  it("edits, copies and exports the reopened recording", async () => {
    const writeText = vi
      .spyOn(navigator.clipboard, "writeText")
      .mockResolvedValue();
    const user = userEvent.setup();
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );

    const segment = await screen.findByDisplayValue("Committed phrase");
    fireEvent.change(segment, { target: { value: "Edited phrase" } });
    fireEvent.blur(segment);
    await waitFor(() =>
      expect(ipc.updateRecorderSegment).toHaveBeenCalledWith(
        1,
        101,
        "Edited phrase",
      ),
    );

    await user.click(screen.getByRole("button", { name: "Copy transcript" }));
    expect(writeText).toHaveBeenCalledWith("Edited phrase");
    await user.click(screen.getByRole("button", { name: "Export WAV" }));
    expect(ipc.exportRecorderWav).toHaveBeenCalledWith(1);
    await user.click(screen.getByRole("button", { name: "Export transcript" }));
    expect(ipc.saveTextDialog).toHaveBeenCalledWith(
      "Edited phrase",
      "Voice note.txt",
    );
  });

  it("reviews, accepts and reverts Recorder polishing without replacing raw segments", async () => {
    const user = userEvent.setup();
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );

    await user.click(screen.getByRole("button", { name: "Polish transcript" }));
    expect(ipc.generateRecorderPolish).toHaveBeenCalledWith(1);
    expect(
      await screen.findByText("Polished committed phrase."),
    ).toBeInTheDocument();
    expect(screen.getByDisplayValue("Committed phrase")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Accept polish" }));
    expect(ipc.acceptRecorderPolish).toHaveBeenCalledWith(
      1,
      "Polished committed phrase.",
    );
    expect(screen.getByText("Polished committed phrase.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Revert polish" }));
    expect(ipc.revertRecorderPolish).toHaveBeenCalledWith(1);
    expect(screen.getByDisplayValue("Committed phrase")).toBeInTheDocument();
  });

  it("discards a polish result when the user switches sessions while it is generated", async () => {
    const user = userEvent.setup();
    let resolvePolish!: (text: string) => void;
    vi.mocked(ipc.listRecorderSessions).mockResolvedValue([
      SESSION,
      SECOND_SESSION,
    ]);
    vi.mocked(ipc.openRecorderSession).mockImplementation(async (id) =>
      id === SECOND_SESSION.id ? SECOND_SESSION : SESSION,
    );
    vi.mocked(ipc.generateRecorderPolish).mockReturnValue(
      new Promise((resolve) => {
        resolvePolish = resolve;
      }),
    );
    renderRecorder();

    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );
    await user.click(screen.getByRole("button", { name: "Polish transcript" }));
    await user.click(screen.getByRole("button", { name: "Open Second note" }));
    await act(async () => resolvePolish("Polish for the first session"));

    expect(screen.getByDisplayValue("Second phrase")).toBeInTheDocument();
    expect(
      screen.queryByText("Polish for the first session"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Accept polish" }),
    ).not.toBeInTheDocument();
  });

  it("does not show an obsolete polish failure after switching sessions", async () => {
    const user = userEvent.setup();
    let rejectPolish!: (reason: Error) => void;
    vi.mocked(ipc.listRecorderSessions).mockResolvedValue([
      SESSION,
      SECOND_SESSION,
    ]);
    vi.mocked(ipc.openRecorderSession).mockImplementation(async (id) =>
      id === SECOND_SESSION.id ? SECOND_SESSION : SESSION,
    );
    vi.mocked(ipc.generateRecorderPolish).mockReturnValue(
      new Promise((_resolve, reject) => {
        rejectPolish = reject;
      }),
    );
    renderRecorder();

    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );
    await user.click(screen.getByRole("button", { name: "Polish transcript" }));
    await user.click(screen.getByRole("button", { name: "Open Second note" }));
    await act(async () =>
      rejectPolish(new Error("obsolete first-session failure")),
    );

    expect(screen.getByDisplayValue("Second phrase")).toBeInTheDocument();
    expect(
      screen.queryByText(/obsolete first-session failure/i),
    ).not.toBeInTheDocument();
  });

  it("disables destructive actions during recording and surfaces failed CRUD", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.openRecorderSession).mockResolvedValue({
      ...SESSION,
      status: "recording",
    });
    vi.mocked(ipc.deleteRecorderSession).mockRejectedValue(
      new Error("Recorder audio cleanup failed"),
    );
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );
    expect(
      screen.getByRole("button", { name: "Delete Voice note" }),
    ).toBeDisabled();

    act(() => sessionHandler?.({ ...SESSION, status: "delete_failed" }));
    await user.click(screen.getByRole("button", { name: "Retry delete" }));
    expect(
      await screen.findByText(/Recorder audio cleanup failed/i),
    ).toBeInTheDocument();
    expect(ipc.openRecorderSession).toHaveBeenCalledTimes(2);
  });
});
