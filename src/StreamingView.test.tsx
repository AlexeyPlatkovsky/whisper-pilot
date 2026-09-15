import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StreamingView } from "./StreamingView";
import * as ipc from "./ipc";
import { readCssBundle } from "./test/readCssBundle";
import type {
  CloudProviderConfiguration,
  StreamingMfu,
  StreamingSession,
  StreamingSessionSummary,
} from "./ipc";

type Handler<T> = (payload: T) => void;

let windowHandler: Handler<
  ipc.StreamingWindow & { session_id: number }
> | null = null;
let sourcesHandler: Handler<ipc.StreamingSources> | null = null;
let endedHandler: Handler<{ session_id: number }> | null = null;
let partialHandler: Handler<ipc.StreamingPartial> | null = null;
let liveCaptureHandler: Handler<ipc.LiveCaptureSnapshot> | null = null;
let liveCaptureRevision = 0;
let writeTextMock: ReturnType<typeof vi.spyOn>;
const revertPrettifyMock = vi.hoisted(() => vi.fn());

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function cloudProviderConfiguration(
  selectedProvider: CloudProviderConfiguration["selected_provider"],
): CloudProviderConfiguration {
  return {
    selected_provider: selectedProvider,
    providers: [
      {
        id: "deepgram",
        name: "Deepgram",
        model: "Nova-3",
        configured: true,
      },
      {
        id: "assemblyai",
        name: "AssemblyAI",
        model: "Universal-3.5 Pro",
        configured: false,
      },
      {
        id: "openai",
        name: "OpenAI",
        model: "GPT Transcribe",
        configured: true,
      },
    ],
  };
}

vi.mock("./ipc", () => ({
  listStreamingSessions: vi.fn(async () => []),
  openStreamingSession: vi.fn(),
  renameStreamingSession: vi.fn(),
  deleteStreamingSession: vi.fn(),
  clearStreamingSession: vi.fn(),
  createStreamingSession: vi.fn(),
  startStreamingSession: vi.fn(),
  stopStreamingSession: vi.fn(),
  getLiveCaptureSnapshot: vi.fn(async () => ({
    phase: "idle" as const,
    session_id: null,
    source: null,
    generation: 0,
    revision: 0,
    error: null,
  })),
  onLiveCaptureState: vi.fn(async (handler: Handler<unknown>) => {
    liveCaptureHandler = handler as Handler<ipc.LiveCaptureSnapshot>;
    return () => {
      liveCaptureHandler = null;
    };
  }),
  generateStreamingMfu: vi.fn(),
  generateStreamingPrettify: vi.fn(),
  acceptStreamingPrettify: vi.fn(),
  revertStreamingPrettify: revertPrettifyMock,
  translateStreamingWindow: vi.fn(),
  listStreamingTranslations: vi.fn(async () => []),
  setStreamingTranslationEnabled: vi.fn(),
  setStreamingTranslationTargetLanguage: vi.fn(),
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
    endedHandler = (payload) => {
      (handler as Handler<{ session_id: number }>)(payload);
      liveCaptureRevision += 1;
      liveCaptureHandler?.({
        phase: "idle",
        session_id: null,
        source: null,
        generation: 1,
        revision: liveCaptureRevision,
        error: null,
      });
    };
    return () => {
      endedHandler = null;
    };
  }),
  onStreamingPartial: vi.fn(async (handler: Handler<unknown>) => {
    partialHandler = handler as Handler<ipc.StreamingPartial>;
    return () => {
      partialHandler = null;
    };
  }),
  onStreamingError: vi.fn(async () => () => {}),
  saveTextDialog: vi.fn(async () => null),
  // WP-96: the MFU panel toggle reads/writes these; default ON with no
  // fields set, matching the shipped Settings default.
  getSettings: vi.fn(async () => ({
    theme: "system",
    ui_language: "en",
    active_model_diarization: "none",
    export_file_type: "plain_text",
  })),
  setSetting: vi.fn(),
  // WP-93: Live Translation's model-readiness check; no model configured by
  // default so most existing tests exercise the switch's disabled state
  // unless a test explicitly opts in.
  listTaskModels: vi.fn(async () => []),
  getCloudProviderConfig: vi.fn(async () => ({
    selected_provider: "deepgram",
    providers: [
      { id: "deepgram", name: "Deepgram", model: "Nova-3", configured: true },
      {
        id: "assemblyai",
        name: "AssemblyAI",
        model: "Universal-3.5 Pro",
        configured: false,
      },
      {
        id: "openai",
        name: "OpenAI",
        model: "GPT Transcribe",
        configured: false,
      },
    ],
  })),
}));

const SESSION_A: StreamingSessionSummary = {
  id: 1,
  title: "Standup",
  created_at_ms: 100,
  updated_at_ms: 100,
  status: "stopped",
  translation_enabled: false,
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
    translation_enabled: false,
    windows: [],
    ...overrides,
  };
}

const MFU: StreamingMfu = {
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

// This split keeps a shared mock surface; a scenario may only use part of it.
void [writeTextMock];

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(ipc.openStreamingSession).mockReset();
  windowHandler = null;
  sourcesHandler = null;
  endedHandler = null;
  partialHandler = null;
  liveCaptureHandler = null;
  liveCaptureRevision = 0;
  vi.mocked(ipc.getLiveCaptureSnapshot).mockResolvedValue({
    phase: "idle",
    session_id: null,
    source: null,
    generation: 0,
    revision: 0,
    error: null,
  });
  vi.mocked(ipc.getCloudProviderConfig)
    .mockReset()
    .mockResolvedValue(cloudProviderConfiguration("deepgram"));
  const startMock = vi.mocked(ipc.startStreamingSession);
  startMock.mockResolvedValue = ((summary: StreamingSessionSummary) =>
    startMock.mockImplementation(async () => {
      liveCaptureRevision += 1;
      liveCaptureHandler?.({
        phase: "capturing",
        session_id: summary.id,
        source: "streaming",
        generation: 1,
        revision: liveCaptureRevision,
        error: null,
      });
      return summary;
    })) as typeof startMock.mockResolvedValue;
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
  it("keeps capture-sensitive controls fail-closed until lifecycle hydration completes", async () => {
    let resolveSnapshot!: (snapshot: ipc.LiveCaptureSnapshot) => void;
    vi.mocked(ipc.getLiveCaptureSnapshot).mockReturnValue(
      new Promise((resolve) => {
        resolveSnapshot = resolve;
      }),
    );

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await waitFor(() => expect(liveCaptureHandler).not.toBeNull());

    expect.soft(screen.getByRole("button", { name: "Start" })).toBeDisabled();
    expect
      .soft(screen.getByRole("button", { name: "New meeting" }))
      .toBeDisabled();
    expect
      .soft(screen.getByRole("button", { name: "Use cloud transcription" }))
      .toBeDisabled();
    expect
      .soft(screen.getByRole("button", { name: "Settings" }))
      .toBeDisabled();

    resolveSnapshot({
      phase: "idle",
      session_id: null,
      source: null,
      generation: 0,
      revision: 0,
      error: null,
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Start" })).toBeEnabled(),
    );
    expect(screen.getByRole("button", { name: "New meeting" })).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Use cloud transcription" }),
    ).toBeEnabled();
    expect(screen.getByRole("button", { name: "Settings" })).toBeEnabled();
  });

  it("enables Stop as soon as the backend reports Starting", async () => {
    vi.mocked(ipc.startStreamingSession).mockReturnValue(new Promise(() => {}));
    const user = userEvent.setup();
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await waitFor(() => expect(liveCaptureHandler).not.toBeNull());
    await user.click(await screen.findByRole("button", { name: "Start" }));
    act(() => {
      liveCaptureRevision += 1;
      liveCaptureHandler!({
        phase: "starting",
        session_id: 44,
        source: "streaming",
        generation: 1,
        revision: liveCaptureRevision,
        error: null,
      });
    });

    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
  });

  it("rehydrates an active backend capture after a renderer remount", async () => {
    vi.mocked(ipc.getLiveCaptureSnapshot).mockResolvedValue({
      phase: "capturing",
      session_id: 41,
      source: "streaming",
      generation: 3,
      revision: 12,
      error: null,
    });
    vi.mocked(ipc.openStreamingSession)
      .mockResolvedValueOnce(
        openedSession({
          id: 41,
          title: "Active capture",
          status: "active",
        }),
      )
      .mockResolvedValueOnce(
        openedSession({
          id: 41,
          title: "Active capture",
          status: "active",
          windows: ONE_WINDOW,
        }),
      );

    const first = render(
      <StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />,
    );
    expect(await screen.findByRole("button", { name: "Stop" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
    await waitFor(() =>
      expect(ipc.openStreamingSession).toHaveBeenCalledWith(41),
    );
    first.unmount();

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    expect(await screen.findByRole("button", { name: "Stop" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
    expect(
      await screen.findByText("hello there", { exact: false }),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(document.querySelector(".wp-status-timer")).toHaveTextContent(
        "00:07",
      ),
    );
    expect(ipc.openStreamingSession).toHaveBeenLastCalledWith(41);

    await waitFor(() => expect(liveCaptureHandler).not.toBeNull());
    act(() => {
      liveCaptureHandler!({
        phase: "idle",
        session_id: null,
        source: null,
        generation: 3,
        revision: 13,
        error: null,
      });
    });
    expect(await screen.findByRole("button", { name: "Resume" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
  });

  it("opens the first persisted session on mount", async () => {
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.openStreamingSession).mockResolvedValue(
      openedSession({ windows: ONE_WINDOW }),
    );

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    expect(await screen.findByText("Standup")).toBeInTheDocument();
    await waitFor(() =>
      expect(ipc.openStreamingSession).toHaveBeenCalledWith(SESSION_A.id),
    );
    expect(await screen.findByText("hello there")).toBeInTheDocument();
  });

  it("keeps a manual selection when the automatic first open resolves late", async () => {
    const initial = deferred<StreamingSession>();
    const second = { ...SESSION_A, id: 2, title: "Review" };
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A, second]);
    vi.mocked(ipc.openStreamingSession).mockImplementation((id) =>
      id === SESSION_A.id
        ? initial.promise
        : Promise.resolve(
            openedSession({
              id: second.id,
              title: second.title,
              windows: [{ ...ONE_WINDOW[0], text: "selected review" }],
            }),
          ),
    );
    const user = userEvent.setup();

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await waitFor(() =>
      expect(ipc.openStreamingSession).toHaveBeenCalledWith(SESSION_A.id),
    );
    await user.click(screen.getByRole("button", { name: "Open Review" }));
    expect(await screen.findByText("selected review")).toBeInTheDocument();

    await act(async () => initial.resolve(openedSession()));
    expect(screen.getByText("selected review")).toBeInTheDocument();
  });

  it("ignores a late automatic-open error after a manual selection succeeds", async () => {
    const initial = deferred<StreamingSession>();
    const second = { ...SESSION_A, id: 2, title: "Review" };
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A, second]);
    vi.mocked(ipc.openStreamingSession).mockImplementation((id) =>
      id === SESSION_A.id
        ? initial.promise
        : Promise.resolve(
            openedSession({
              id: second.id,
              title: second.title,
              windows: ONE_WINDOW,
            }),
          ),
    );
    const user = userEvent.setup();

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await waitFor(() =>
      expect(ipc.openStreamingSession).toHaveBeenCalledWith(SESSION_A.id),
    );
    await user.click(screen.getByRole("button", { name: "Open Review" }));
    expect(
      await screen.findByRole("heading", { name: second.title }),
    ).toBeInTheDocument();

    await act(async () => initial.reject(new Error("stale automatic open")));
    expect(screen.queryByText(/stale automatic open/)).toBeNull();
    expect(
      screen.getByRole("heading", { name: second.title }),
    ).toBeInTheDocument();
  });

  it("does not start automatic selection after the user chooses a session before lifecycle hydration", async () => {
    const lifecycle = deferred<ipc.LiveCaptureSnapshot>();
    const selected = deferred<StreamingSession>();
    const second = { ...SESSION_A, id: 2, title: "Review" };
    vi.mocked(ipc.getLiveCaptureSnapshot).mockReturnValue(lifecycle.promise);
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A, second]);
    vi.mocked(ipc.openStreamingSession).mockImplementation((id) =>
      id === second.id
        ? selected.promise
        : Promise.resolve(openedSession({ id: SESSION_A.id })),
    );
    const user = userEvent.setup();

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(
      await screen.findByRole("button", { name: "Open Review" }),
    );
    await waitFor(() =>
      expect(ipc.openStreamingSession).toHaveBeenCalledWith(second.id),
    );

    lifecycle.resolve({
      phase: "idle",
      session_id: null,
      source: null,
      generation: 0,
      revision: 0,
      error: null,
    });
    await act(async () => Promise.resolve());
    expect(ipc.openStreamingSession).not.toHaveBeenCalledWith(SESSION_A.id);

    await act(async () =>
      selected.resolve(
        openedSession({
          id: second.id,
          title: second.title,
          windows: ONE_WINDOW,
        }),
      ),
    );
    expect(
      await screen.findByRole("heading", { name: second.title }),
    ).toBeInTheDocument();
  });

  it("does not let late active-session rehydration overwrite an explicit selection", async () => {
    const active = deferred<StreamingSession>();
    const second = { ...SESSION_A, id: 2, title: "Review" };
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A, second]);
    vi.mocked(ipc.getLiveCaptureSnapshot).mockResolvedValue({
      phase: "capturing",
      session_id: 41,
      source: "streaming",
      generation: 3,
      revision: 12,
      error: null,
    });
    vi.mocked(ipc.openStreamingSession).mockImplementation((id) =>
      id === 41
        ? active.promise
        : Promise.resolve(
            openedSession({
              id: second.id,
              title: second.title,
              windows: ONE_WINDOW,
            }),
          ),
    );
    const user = userEvent.setup();

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await waitFor(() =>
      expect(ipc.openStreamingSession).toHaveBeenCalledWith(41),
    );
    await user.click(
      await screen.findByRole("button", { name: "Open Review" }),
    );
    expect(
      await screen.findByRole("heading", { name: second.title }),
    ).toBeInTheDocument();

    await act(async () =>
      active.resolve(
        openedSession({ id: 41, title: "Active capture", status: "active" }),
      ),
    );
    expect(
      screen.getByRole("heading", { name: second.title }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Active capture" }),
    ).toBeNull();
  });

  it("keeps live-capture identity on session A when the user opens stopped session B", async () => {
    const user = userEvent.setup();
    const duo = { ...SESSION_A, title: "Duo" };
    const review = { ...SESSION_A, id: 2, title: "Review" };
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([duo, review]);
    vi.mocked(ipc.getLiveCaptureSnapshot).mockResolvedValue({
      phase: "capturing",
      session_id: duo.id,
      source: "streaming",
      generation: 1,
      revision: 1,
      error: null,
    });
    vi.mocked(ipc.openStreamingSession).mockImplementation(async (id) =>
      openedSession({
        id,
        title: id === duo.id ? duo.title : review.title,
        status: id === duo.id ? "active" : "stopped",
      }),
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("On Air"),
    );
    await user.click(
      await screen.findByRole("button", { name: "Open Review" }),
    );
    await screen.findByRole("heading", { name: "Review" });

    expect(screen.getByRole("status")).toHaveTextContent("Ready");
    expect(
      within(screen.getByRole("listitem", { name: "Duo" })).getByRole("img"),
    ).toHaveAccessibleName("On Air");
    expect(
      within(screen.getByRole("listitem", { name: "Review" })).getByRole("img"),
    ).toHaveAccessibleName("Ready");
  });

  it("does not display a Recorder capture failure in Streaming", async () => {
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await waitFor(() => expect(liveCaptureHandler).not.toBeNull());

    act(() => {
      liveCaptureHandler!({
        phase: "error",
        session_id: 77,
        source: "recorder",
        generation: 1,
        revision: 1,
        error: "Recorder callback buffer pool was exhausted",
      });
    });

    expect(
      screen.queryByText(/Recorder callback buffer pool was exhausted/i),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Ready");
  });

  it("filters sessions by title only after three characters and shows no matches", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([
      SESSION_A,
      { ...SESSION_A, id: 2, title: "Retro" },
    ]);

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    const search = await screen.findByLabelText("Search meetings");
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
      await screen.findByText(/Start a meeting, or open one/),
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
      translation_enabled: false,
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByRole("button", { name: "Start" }));

    expect(
      await screen.findByRole("button", { name: "Stop" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Listening…")).toBeInTheDocument();
  });

  it("replaces the centered listening placeholder with the first live partial", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "First partial session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
      translation_enabled: false,
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(partialHandler).not.toBeNull());
    act(() => {
      partialHandler!({
        session_id: 2,
        item_id: null,
        text: "The first provisional phrase",
      });
    });

    expect(screen.queryByText("Listening…")).not.toBeInTheDocument();
    expect(document.querySelector(".wp-streaming-partial")).toHaveTextContent(
      "The first provisional phrase",
    );
  });

  it("keeps the listening placeholder when a live partial contains no text", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "Empty partial session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
      translation_enabled: false,
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(partialHandler).not.toBeNull());
    act(() => {
      partialHandler!({ session_id: 2, item_id: null, text: "   " });
    });

    expect(screen.getByText("Listening…")).toBeInTheDocument();
    expect(document.querySelector(".wp-streaming-partial")).toBeNull();
  });

  it("keeps a newer Cloud partial visible and clears it only when its own final turn arrives", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "Cloud session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
      translation_enabled: false,
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(partialHandler).not.toBeNull());

    act(() => {
      partialHandler!({
        session_id: 2,
        item_id: "turn-2",
        text: "newer words",
      });
      windowHandler!({
        session_id: 2,
        item_id: "turn-1",
        window_index: 0,
        start_ms: 0,
        end_ms: 7_000,
        text: "older final",
        language: "en",
        outcome_ok: true,
      });
    });
    expect(document.querySelector(".wp-streaming-partial")).toHaveTextContent(
      "newer words",
    );
    const committed = document.querySelector(".streaming-window");
    const partial = document.querySelector(".wp-streaming-partial");
    expect(committed).not.toBeNull();
    expect(partial).not.toBeNull();
    expect(
      committed!.compareDocumentPosition(partial!) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);

    act(() => {
      windowHandler!({
        session_id: 2,
        item_id: "turn-2",
        window_index: 1,
        start_ms: 7_000,
        end_ms: 14_000,
        text: "newer words",
        language: "en",
        outcome_ok: true,
      });
    });
    expect(document.querySelector(".wp-streaming-partial")).toBeNull();
  });

  it("keeps a revised partial entirely italic until it is committed", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "Local session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
      translation_enabled: false,
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(partialHandler).not.toBeNull());

    act(() => {
      partialHandler!({
        session_id: 2,
        item_id: null,
        text: "Так давай попробуем снова",
      });
      partialHandler!({
        session_id: 2,
        item_id: null,
        text: "Так давай попробуем ещё раз",
      });
    });

    const partial = screen.getByText("Так давай попробуем ещё раз");
    expect(partial.tagName).toBe("EM");
    expect(document.querySelector(".wp-streaming-partial-stable")).toBeNull();
  });

  it("follows the latest phrase, pauses after scrolling up, and resumes at the bottom", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "Autoscroll session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
      translation_enabled: false,
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());

    const transcript = document.querySelector(
      ".wp-transcript-content",
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
      windowHandler!({
        session_id: 2,
        window_index: 0,
        start_ms: 0,
        end_ms: 7_000,
        text: "first phrase",
        language: "en",
        outcome_ok: true,
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
      partialHandler!({
        session_id: 2,
        item_id: "turn-2",
        text: "do not follow me yet",
      });
    });
    expect(scrollTop).toBe(100);

    scrollTop = 700;
    fireEvent.scroll(transcript);
    scrollHeight = 1_200;
    act(() => {
      partialHandler!({
        session_id: 2,
        item_id: "turn-2",
        text: "following again",
      });
    });
    expect(scrollTop).toBe(1_200);

    const styles = readCssBundle();
    expect(styles).toMatch(
      /\.wp-streaming-autoscroll-tail\s*\{[^}]*flex:\s*0 0 22%;/s,
    );
  });

  it("insets Streaming transcript content from both panel borders", async () => {
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await screen.findByText(/Start a meeting, or open one/);

    const transcript = document.querySelector(".wp-transcript-content");
    expect(transcript).not.toBeNull();
    expect(transcript).toHaveClass("wp-transcript-content--inset");

    const styles = readCssBundle();
    expect(styles).toMatch(
      /\.wp-transcript-content--inset\s*\{[^}]*padding-left:\s*var\(--wp-space-md\);[^}]*padding-right:\s*var\(--wp-space-md\);/s,
    );
  });

  it("keeps shared Meeting and Streaming MFU content vertically scrollable", () => {
    const styles = readCssBundle();
    expect(styles).toMatch(/\.wp-mfu\s*\{[^}]*overflow:\s*hidden;/s);
    expect(styles).toMatch(
      /\.wp-mfu-content\s*\{[^}]*min-height:\s*0;[^}]*overflow-y:\s*auto;/s,
    );
  });

  it("appends a live window for the active session as it arrives", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 2,
      title: "New Streaming Session",
      created_at_ms: 200,
      updated_at_ms: 200,
      status: "active",
      translation_enabled: false,
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
      translation_enabled: false,
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
      translation_enabled: false,
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
      translation_enabled: false,
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
      translation_enabled: false,
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
      translation_enabled: false,
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
      translation_enabled: false,
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
            text: "past meeting mfu",
            language: "en",
            outcome_ok: true,
          },
        ],
      }),
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));

    expect(await screen.findByText(/past meeting mfu/)).toBeInTheDocument();
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

      await user.click(
        await screen.findByRole("button", { name: "Open Standup" }),
      );

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
        translation_enabled: false,
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
        translation_enabled: false,
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

    // State transition: New creates an inactive session; only Start moves it
    // into the active, capturing state.
    it("the New icon creates an inactive session without starting capture", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.createStreamingSession).mockResolvedValue({
        id: 2,
        title: "New Streaming Session",
        created_at_ms: 200,
        updated_at_ms: 200,
        status: "stopped",
        translation_enabled: false,
      });
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 2,
        title: "New Streaming Session",
        created_at_ms: 200,
        updated_at_ms: 201,
        status: "active",
        translation_enabled: false,
      });
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      expect(await screen.findByText(/hello there/)).toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "New meeting" }));

      expect(ipc.createStreamingSession).toHaveBeenCalledOnce();
      expect(ipc.startStreamingSession).not.toHaveBeenCalled();
      expect(screen.queryByText(/hello there/)).not.toBeInTheDocument();
      expect(
        await screen.findByRole("button", { name: "Start" }),
      ).toBeEnabled();
      expect(screen.getByRole("status")).toHaveTextContent("Ready");

      await user.click(screen.getByRole("button", { name: "Start" }));

      expect(ipc.startStreamingSession).toHaveBeenCalledWith(2);
      expect(await screen.findByText("Listening…")).toBeInTheDocument();
    });

    it("does not let a stale initial list erase a session created while hydration is pending", async () => {
      const user = userEvent.setup();
      const initialList = deferred<StreamingSessionSummary[]>();
      const postCreateList = deferred<StreamingSessionSummary[]>();
      const created: StreamingSessionSummary = {
        id: 2,
        title: "Fresh meeting",
        created_at_ms: 200,
        updated_at_ms: 200,
        status: "stopped",
        translation_enabled: false,
      };
      vi.mocked(ipc.listStreamingSessions)
        .mockReturnValueOnce(initialList.promise)
        .mockReturnValueOnce(postCreateList.promise);
      vi.mocked(ipc.createStreamingSession).mockResolvedValue(created);
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(
        await screen.findByRole("button", { name: "New meeting" }),
      );
      await waitFor(() =>
        expect(ipc.listStreamingSessions).toHaveBeenCalledTimes(2),
      );
      postCreateList.resolve([created]);
      expect(await screen.findByText("Fresh meeting")).toBeInTheDocument();

      // The launch request started before the user-created item existed.
      // Resolving it last must not replace the newer library state.
      initialList.resolve([]);
      await act(async () => {});
      expect(
        screen.getByRole("button", { name: "Open Fresh meeting" }),
      ).toBeInTheDocument();
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
      const dialog = screen.getByRole("dialog", { name: "Rename meeting" });
      const input = within(dialog).getByRole("textbox", {
        name: "Meeting title",
      });
      await user.clear(input);
      await user.type(input, "Renamed");
      await user.click(within(dialog).getByRole("button", { name: "Save" }));

      expect(ipc.renameStreamingSession).toHaveBeenCalledWith(1, "Renamed");
      expect(
        screen.queryByRole("dialog", { name: "Rename meeting" }),
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
      await user.click(screen.getByRole("button", { name: "Rename meeting" }));
      const dialog = screen.getByRole("dialog", { name: "Rename meeting" });
      const input = within(dialog).getByRole("textbox", {
        name: "Meeting title",
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
      const dialog = screen.getByRole("dialog", { name: "Rename meeting" });
      const input = within(dialog).getByRole("textbox", {
        name: "Meeting title",
      });
      await user.clear(input);
      await user.type(input, "   ");
      await user.click(within(dialog).getByRole("button", { name: "Save" }));
      expect(within(dialog).getByRole("alert")).toHaveTextContent(
        "Meeting title is required",
      );

      await user.clear(input);
      await user.type(input, "a".repeat(121));
      await user.click(within(dialog).getByRole("button", { name: "Save" }));
      expect(within(dialog).getByRole("alert")).toHaveTextContent(
        "Meeting title must be 120 characters or fewer",
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
        screen.getByRole("dialog", { name: "Rename meeting" }),
      ).toBeInTheDocument();

      await user.keyboard("{Escape}");

      expect(
        screen.queryByRole("dialog", { name: "Rename meeting" }),
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
      expect(
        within(dialog).getByRole("button", { name: "Delete" }),
      ).toHaveClass("modal-button--danger");
      await user.click(within(dialog).getByRole("button", { name: "Delete" }));

      expect(ipc.deleteStreamingSession).toHaveBeenCalledWith(1);
      expect(
        screen.queryByRole("alertdialog", { name: "Delete Standup" }),
      ).not.toBeInTheDocument();
    });

    it("clears transcript, MFU, prettify, and translations only after confirmation", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({
          windows: ONE_WINDOW,
          mfu: {
            summary: "Summary to clear",
            decisions: "",
            action_items: "",
            open_questions: "",
            participants: "",
          },
          prettified_text: "Polished text to clear",
          translation_enabled: true,
        }),
      );
      vi.mocked(ipc.clearStreamingSession).mockResolvedValue(
        openedSession({ windows: [], translation_enabled: true }),
      );
      vi.mocked(ipc.translateStreamingWindow).mockResolvedValue(
        "Translated text to clear",
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      const clear = screen.getByRole("button", { name: "Clear meeting" });
      expect(clear.querySelector("svg")).toHaveAttribute("data-icon", "eraser");
      await user.click(clear);
      expect(ipc.clearStreamingSession).not.toHaveBeenCalled();
      const dialog = screen.getByRole("alertdialog", {
        name: "Clear meeting",
      });
      expect(
        within(dialog).getByRole("button", { name: "Clear meeting" }),
      ).toHaveClass("modal-button--danger");
      await user.click(
        within(dialog).getByRole("button", { name: "Clear meeting" }),
      );

      expect(ipc.clearStreamingSession).toHaveBeenCalledWith(SESSION_A.id);
      expect(screen.queryByText("hello there")).not.toBeInTheDocument();
      expect(screen.queryByText("Summary to clear")).not.toBeInTheDocument();
      expect(
        screen.getByText("Start a meeting, or open one from the list."),
      ).toBeInTheDocument();
    });

    it("keeps Clear available for retry when the first clear request is rejected", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.clearStreamingSession)
        .mockRejectedValueOnce(new Error("clear failed"))
        .mockResolvedValueOnce(openedSession({ windows: [] }));
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(screen.getByRole("button", { name: "Clear meeting" }));
      const dialog = screen.getByRole("alertdialog", {
        name: "Clear meeting",
      });
      const confirm = within(dialog).getByRole("button", {
        name: "Clear meeting",
      });
      await user.click(confirm);
      expect(await screen.findByText(/clear failed/)).toBeInTheDocument();
      expect(confirm).toBeEnabled();
      await user.click(confirm);

      await waitFor(() =>
        expect(ipc.clearStreamingSession).toHaveBeenCalledTimes(2),
      );
    });

    it("blocks Clear while Craft MFU can still persist derived content", async () => {
      const craft = deferred<StreamingSession>();
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingMfu).mockReturnValue(craft.promise);
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(screen.getByRole("button", { name: "Craft MFU" }));

      expect(
        screen.getByRole("button", { name: "Clear meeting" }),
      ).toBeDisabled();
      await act(async () =>
        craft.resolve(openedSession({ windows: ONE_WINDOW, mfu: MFU })),
      );
    });

    it("blocks Clear while Prettify can still return derived content", async () => {
      const prettify = deferred<string>();
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockReturnValue(
        prettify.promise,
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(
        screen.getByRole("button", { name: "Prettify transcript" }),
      );

      expect(
        screen.getByRole("button", { name: "Clear meeting" }),
      ).toBeDisabled();
      await act(async () => prettify.resolve("Prettified candidate"));
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

      await user.click(screen.getByRole("button", { name: "Delete meeting" }));

      expect(
        screen.getByRole("alertdialog", { name: "Delete Standup" }),
      ).toBeInTheDocument();
    });
  });
});
