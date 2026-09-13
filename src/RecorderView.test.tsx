import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
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
  is_draft?: boolean;
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
let recorderErrorHandler: Handler<{
  session_id?: number;
  message: string;
}> | null = null;

const { createRecorderDraftMock } = vi.hoisted(() => ({
  createRecorderDraftMock: vi.fn(),
}));

vi.mock("./ipc", () => ({
  createRecorderDraft: createRecorderDraftMock,
  listRecorderSessions: vi.fn(async () => []),
  openRecorderSession: vi.fn(),
  renameRecorderSession: vi.fn(),
  deleteRecorderSession: vi.fn(),
  clearRecorderTranscript: vi.fn(),
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
  onRecorderError: vi.fn(async (handler: Handler<unknown>) => {
    recorderErrorHandler = handler as typeof recorderErrorHandler;
    return () => {
      recorderErrorHandler = null;
    };
  }),
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

const DRAFT_SESSION: RecorderSession = {
  ...SESSION,
  id: 3,
  title: "Recording draft",
  duration_ms: 0,
  segments: [],
  is_draft: true,
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
  recorderErrorHandler = null;
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
  createRecorderDraftMock.mockResolvedValue(DRAFT_SESSION);
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
  vi.mocked(ipc.clearRecorderTranscript).mockResolvedValue({
    ...SESSION,
    segments: [],
  });
  if (!navigator.clipboard) {
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: async () => {} },
      configurable: true,
    });
  }
});

describe("Recorder mode", () => {
  it("creates and selects a durable recording draft as soon as plus is clicked", async () => {
    const user = userEvent.setup();
    renderRecorder();

    await user.click(
      await screen.findByRole("button", { name: "New recording" }),
    );

    expect(createRecorderDraftMock).toHaveBeenCalledOnce();
    expect(
      screen.getByRole("button", { name: "Open Recording draft" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Recording draft" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Rename recording" }),
    ).toBeEnabled();

    await user.click(screen.getByRole("button", { name: "Start" }));
    expect(ipc.startRecorderSession).toHaveBeenCalledWith(3);
  });

  it("guards Recorder Start synchronously while permission preflight is pending", async () => {
    let resolvePermission!: (value: ipc.MicrophonePermissionStatus) => void;
    vi.mocked(ipc.getMicrophonePermissionStatus).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolvePermission = resolve;
        }),
    );
    renderRecorder();
    const start = await screen.findByRole("button", { name: "Start" });
    await waitFor(() => expect(start).toBeEnabled());

    act(() => {
      start.click();
      start.click();
    });

    expect(ipc.getMicrophonePermissionStatus).toHaveBeenCalledOnce();
    expect(start).toBeDisabled();
    await act(async () => resolvePermission("authorized"));
    await waitFor(() =>
      expect(ipc.startRecorderSession).toHaveBeenCalledOnce(),
    );
  });

  it("keeps Recorder Start pending when the workspace unmounts during preflight", async () => {
    let resolvePermission!: (value: ipc.MicrophonePermissionStatus) => void;
    vi.mocked(ipc.getMicrophonePermissionStatus).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolvePermission = resolve;
        }),
    );
    function Harness() {
      const [recorderVisible, setRecorderVisible] = useState(true);
      const [startPending, setStartPending] = useState(false);
      return recorderVisible ? (
        <RecorderView
          onSelectMeeting={() => setRecorderVisible(false)}
          onSelectStreaming={() => {}}
          onOpenSettings={() => {}}
          meetingTranscriptionActive={false}
          recorderStartPending={startPending}
          onRecorderStartPendingChange={setStartPending}
        />
      ) : (
        <button type="button" onClick={() => setRecorderVisible(true)}>
          Return to Recorder
        </button>
      );
    }
    const user = userEvent.setup();
    render(<Harness />);
    const start = await screen.findByRole("button", { name: "Start" });
    await waitFor(() => expect(start).toBeEnabled());
    await user.click(start);
    await user.click(screen.getByRole("button", { name: "Meeting" }));
    await user.click(
      screen.getByRole("button", { name: "Return to Recorder" }),
    );

    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
    await act(async () => resolvePermission("authorized"));
    await waitFor(() =>
      expect(ipc.startRecorderSession).toHaveBeenCalledOnce(),
    );
  });

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
      await screen.findByRole("textbox", {
        name: "Transcript segment 101",
      }),
    ).toHaveTextContent("Committed phrase");

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

  it("uses the shared header and searchable library layout", async () => {
    vi.mocked(ipc.listRecorderSessions).mockResolvedValue([
      SESSION,
      SECOND_SESSION,
    ]);
    const user = userEvent.setup();
    renderRecorder();

    expect(
      await screen.findByRole("button", { name: "Toggle sidebar" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "New recording" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Settings" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();

    await user.type(
      screen.getByRole("searchbox", { name: "Search recordings" }),
      "Second",
    );
    expect(
      screen.getByRole("button", { name: "Open Second note" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Open Voice note" }),
    ).not.toBeInTheDocument();
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
      partialHandler?.({
        session_id: 1,
        revision: 1,
        text: "Так давай попробуем снова",
      });
      partialHandler?.({
        session_id: 1,
        revision: 2,
        text: "Так давай попробуем ещё раз",
      });
      partialHandler?.({ session_id: 1, revision: 1, text: "stale partial" });
    });
    expect(
      screen.getByRole("textbox", { name: "Transcript segment 101" }),
    ).toHaveTextContent("Committed phrase");
    expect(screen.getByText("Так давай попробуем")).toHaveClass(
      "wp-streaming-partial-stable",
    );
    expect(screen.getByText("ещё раз").closest("em")).not.toBeNull();
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
    expect(
      screen.getByRole("textbox", { name: "Transcript segment 101" }),
    ).toHaveTextContent("Committed phrase");
    expect(
      screen.getByRole("textbox", { name: "Transcript segment 102" }),
    ).toHaveTextContent("Committed second phrase");
    expect(screen.queryByText("ещё раз")).not.toBeInTheDocument();
  });

  it("renders committed phrases as one Streaming-style transcript flow", async () => {
    const user = userEvent.setup();
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );

    const transcript = screen.getByLabelText("Recorder transcript");
    expect(transcript.querySelector("textarea")).toBeNull();
    expect(
      transcript.querySelector(".streaming-transcript-text"),
    ).not.toBeNull();
    expect(
      screen.getByRole("textbox", { name: "Transcript segment 101" }),
    ).toHaveTextContent("Committed phrase");
  });

  it("follows live Recorder text, pauses after scrolling up, and resumes at the bottom", async () => {
    const user = userEvent.setup();
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );
    await waitFor(() => expect(committedHandler).not.toBeNull());

    const transcript = screen.getByLabelText(
      "Recorder transcript",
    ) as HTMLDivElement;
    let scrollHeight = 1_000;
    let scrollTop = 0;
    Object.defineProperties(transcript, {
      clientHeight: { configurable: true, get: () => 400 },
      scrollHeight: { configurable: true, get: () => scrollHeight },
      scrollTop: {
        configurable: true,
        get: () => scrollTop,
        set: (value: number) => {
          scrollTop = value;
        },
      },
    });

    act(() => {
      sessionHandler?.({ ...SESSION, status: "recording" });
      liveHandler?.({
        phase: "capturing",
        session_id: 1,
        source: "recorder",
        generation: 1,
        revision: 2,
        error: null,
      });
      committedHandler?.({
        session_id: 1,
        id: 102,
        start_sample: 24_000,
        end_sample: 48_000,
        text: "Newest phrase",
        language: "en",
      });
    });
    expect(scrollTop).toBe(1_000);
    expect(
      document.querySelector(".wp-streaming-autoscroll-tail"),
    ).not.toBeNull();

    scrollTop = 100;
    fireEvent.scroll(transcript);
    scrollHeight = 1_100;
    act(() => {
      partialHandler?.({
        session_id: 1,
        revision: 1,
        text: "do not follow me yet",
      });
    });
    expect(scrollTop).toBe(100);

    scrollTop = 700;
    fireEvent.scroll(transcript);
    scrollHeight = 1_200;
    act(() => {
      partialHandler?.({
        session_id: 1,
        revision: 2,
        text: "following again",
      });
    });
    expect(scrollTop).toBe(1_200);

    act(() => {
      liveHandler?.({
        phase: "stopping",
        session_id: 1,
        source: "recorder",
        generation: 1,
        revision: 3,
        error: null,
      });
    });
    scrollHeight = 1_300;
    act(() => {
      committedHandler?.({
        session_id: 1,
        id: 103,
        start_sample: 48_000,
        end_sample: 72_000,
        text: "Final phrase",
        language: "en",
      });
    });
    expect(scrollTop).toBe(1_300);
    expect(
      document.querySelector(".wp-streaming-autoscroll-tail"),
    ).not.toBeNull();
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
    expect(screen.getByRole("status")).toHaveClass("wp-status--error");
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
      await screen.findByRole("textbox", {
        name: "Transcript segment 101",
      }),
    ).toHaveTextContent("Restored committed phrase");
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

  it("scopes a Recorder callback failure to its failed session when another recording is opened", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listRecorderSessions).mockResolvedValue([
      SESSION,
      SECOND_SESSION,
    ]);
    vi.mocked(ipc.openRecorderSession).mockImplementation(async (id) =>
      id === SECOND_SESSION.id ? SECOND_SESSION : SESSION,
    );
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );
    await waitFor(() => expect(recorderErrorHandler).not.toBeNull());

    act(() => {
      liveHandler!({
        phase: "error",
        session_id: SESSION.id,
        source: "recorder",
        generation: 1,
        revision: 1,
        error: "Recorder callback buffer pool was exhausted",
      });
      recorderErrorHandler!({
        session_id: SESSION.id,
        message: "Recorder callback buffer pool was exhausted",
      });
    });
    expect(screen.getByRole("status")).toHaveTextContent("Error");

    await user.click(screen.getByRole("button", { name: "Open Second note" }));
    await screen.findByRole("textbox", { name: "Transcript segment 202" });

    expect(screen.getByRole("status")).toHaveTextContent("Completed");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      within(screen.getByRole("listitem", { name: "Voice note" })).getByRole(
        "img",
      ),
    ).toHaveAccessibleName("Error");
    expect(
      within(screen.getByRole("listitem", { name: "Second note" })).getByRole(
        "img",
      ),
    ).toHaveAccessibleName("Completed");
  });

  it("edits, copies and exports the reopened recording", async () => {
    let resolveUpdate!: (segment: RecorderSegment) => void;
    vi.mocked(ipc.updateRecorderSegment).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveUpdate = resolve;
        }),
    );
    const writeText = vi
      .spyOn(navigator.clipboard, "writeText")
      .mockResolvedValue();
    const user = userEvent.setup();
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );

    const segment = await screen.findByRole("textbox", {
      name: "Transcript segment 101",
    });
    fireEvent.focus(segment);
    segment.textContent = "Edited phrase";
    fireEvent.input(segment);
    expect(
      screen.getByRole("button", { name: "Prettify transcript" }),
    ).toBeDisabled();
    fireEvent.blur(segment);

    await user.click(screen.getByRole("button", { name: "Copy transcript" }));
    expect(ipc.updateRecorderSegment).toHaveBeenCalledWith(
      1,
      101,
      "Edited phrase",
    );
    expect(writeText).toHaveBeenCalledWith("Edited phrase");
    await user.click(screen.getByRole("button", { name: "Export WAV" }));
    expect(ipc.exportRecorderWav).toHaveBeenCalledWith(1);
    await user.click(screen.getByRole("button", { name: "Export transcript" }));
    expect(ipc.saveTextDialog).toHaveBeenCalledWith(
      "Edited phrase",
      "Voice note.txt",
    );
    await act(async () =>
      resolveUpdate({ ...FIRST_SEGMENT, text: "Edited phrase" }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Prettify transcript" }),
      ).toBeEnabled(),
    );
  });

  it("restores persisted segment text when an inline edit cannot be saved", async () => {
    vi.mocked(ipc.updateRecorderSegment).mockRejectedValueOnce(
      new Error("save failed"),
    );
    const user = userEvent.setup();
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );

    const segment = await screen.findByRole("textbox", {
      name: "Transcript segment 101",
    });
    segment.textContent = "Unsaved phrase";
    fireEvent.input(segment);
    fireEvent.blur(segment);

    expect(await screen.findByRole("alert")).toHaveTextContent("save failed");
    expect(segment).toHaveTextContent("Committed phrase");
  });

  it("serializes repeated saves of one segment so the newest edit wins", async () => {
    const resolvers: Array<(segment: RecorderSegment) => void> = [];
    vi.mocked(ipc.updateRecorderSegment).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvers.push(resolve);
        }),
    );
    const user = userEvent.setup();
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );

    let segment = await screen.findByRole("textbox", {
      name: "Transcript segment 101",
    });
    segment.textContent = "First edit";
    fireEvent.input(segment);
    fireEvent.blur(segment);
    segment = screen.getByRole("textbox", { name: "Transcript segment 101" });
    segment.textContent = "Newest edit";
    fireEvent.input(segment);
    fireEvent.blur(segment);

    await waitFor(() =>
      expect(ipc.updateRecorderSegment).toHaveBeenCalledTimes(1),
    );
    await act(async () =>
      resolvers[0]({ ...FIRST_SEGMENT, text: "First edit" }),
    );
    await waitFor(() =>
      expect(ipc.updateRecorderSegment).toHaveBeenCalledTimes(2),
    );
    expect(
      screen.getByRole("textbox", { name: "Transcript segment 101" }),
    ).toHaveTextContent("Newest edit");
    await act(async () =>
      resolvers[1]({ ...FIRST_SEGMENT, text: "Newest edit" }),
    );
    expect(
      screen.getByRole("textbox", { name: "Transcript segment 101" }),
    ).toHaveTextContent("Newest edit");
  });

  it("copies and exports the same paragraph flow that Recorder displays", async () => {
    const writeText = vi
      .spyOn(navigator.clipboard, "writeText")
      .mockResolvedValue();
    vi.mocked(ipc.openRecorderSession).mockResolvedValueOnce({
      ...SESSION,
      segments: [
        FIRST_SEGMENT,
        { ...FIRST_SEGMENT, id: 102, text: "Second phrase" },
      ],
    });
    const user = userEvent.setup();
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );

    const flow = screen.getByLabelText("Recorder transcript");
    expect(flow).toHaveTextContent("Committed phrase Second phrase");
    await user.click(screen.getByRole("button", { name: "Copy transcript" }));
    expect(writeText).toHaveBeenCalledWith("Committed phrase Second phrase");
    await user.click(screen.getByRole("button", { name: "Export transcript" }));
    expect(ipc.saveTextDialog).toHaveBeenCalledWith(
      "Committed phrase Second phrase",
      "Voice note.txt",
    );
  });

  it("prettifies in place and can restore the raw transcript", async () => {
    const user = userEvent.setup();
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );

    await user.click(
      screen.getByRole("button", { name: "Prettify transcript" }),
    );
    expect(ipc.generateRecorderPolish).toHaveBeenCalledWith(1);
    await waitFor(() =>
      expect(ipc.acceptRecorderPolish).toHaveBeenCalledWith(
        1,
        "Polished committed phrase.",
      ),
    );
    expect(screen.getByLabelText("Recorder transcript")).toHaveTextContent(
      "Polished committed phrase.",
    );
    expect(
      screen.queryByRole("textbox", { name: "Transcript segment 101" }),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Restore original transcript" }),
    );
    expect(ipc.revertRecorderPolish).toHaveBeenCalledWith(1);
    expect(
      screen.getByRole("textbox", { name: "Transcript segment 101" }),
    ).toHaveTextContent("Committed phrase");
  });

  it("clears the transcript only after confirmation", async () => {
    const user = userEvent.setup();
    renderRecorder();
    await user.click(
      await screen.findByRole("button", { name: "Open Voice note" }),
    );

    await user.click(screen.getByRole("button", { name: "Clear transcript" }));
    expect(ipc.clearRecorderTranscript).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Clear" }));

    expect(ipc.clearRecorderTranscript).toHaveBeenCalledWith(1);
    expect(
      screen.queryByRole("textbox", { name: "Transcript segment 101" }),
    ).not.toBeInTheDocument();
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
    await user.click(
      screen.getByRole("button", { name: "Prettify transcript" }),
    );
    await user.click(screen.getByRole("button", { name: "Open Second note" }));
    await act(async () => resolvePolish("Polish for the first session"));

    expect(
      screen.getByRole("textbox", { name: "Transcript segment 202" }),
    ).toHaveTextContent("Second phrase");
    expect(
      screen.queryByText("Polish for the first session"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("Polish for the first session"),
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
    await user.click(
      screen.getByRole("button", { name: "Prettify transcript" }),
    );
    await user.click(screen.getByRole("button", { name: "Open Second note" }));
    await act(async () =>
      rejectPolish(new Error("obsolete first-session failure")),
    );

    expect(
      screen.getByRole("textbox", { name: "Transcript segment 202" }),
    ).toHaveTextContent("Second phrase");
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
      screen.getByRole("button", { name: "Delete recording" }),
    ).toBeDisabled();

    act(() => sessionHandler?.({ ...SESSION, status: "delete_failed" }));
    await user.click(screen.getByRole("button", { name: "Retry delete" }));
    expect(
      await screen.findByText(/Recorder audio cleanup failed/i),
    ).toBeInTheDocument();
    expect(ipc.openRecorderSession).toHaveBeenCalledTimes(2);
  });
});
