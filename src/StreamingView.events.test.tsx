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
let staleEndedHandler: Handler<{ session_id: number }> | null = null;
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
    staleEndedHandler = handler as Handler<{ session_id: number }>;
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
void [fireEvent, readCssBundle, partialHandler, deferred, MFU];

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

describe("StreamingView — events", () => {
  it("calls onClose when the sidebar Meeting toggle is clicked", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<StreamingView onClose={onClose} onOpenSettings={vi.fn()} />);

    await user.click(
      await screen.findByRole("button", { name: "Transcription" }),
    );

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

  it("cancels deferred registration before later listeners and ignores stale callbacks", async () => {
    const windowRegistration = deferred<() => void>();
    const unlisten = vi.fn();
    const handlers: {
      window: Handler<ipc.StreamingWindow & { session_id: number }> | null;
    } = { window: null };
    vi.mocked(ipc.onStreamingWindow).mockImplementationOnce((handler) => {
      handlers.window = handler;
      return windowRegistration.promise;
    });

    const { unmount } = render(
      <StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />,
    );
    unmount();
    windowRegistration.resolve(unlisten);

    await waitFor(() => expect(unlisten).toHaveBeenCalledOnce());
    expect(ipc.onStreamingSources).not.toHaveBeenCalled();

    handlers.window?.({
      session_id: 1,
      window_index: 0,
      start_ms: 0,
      end_ms: 1,
      language: "en",
      text: "must be ignored",
      outcome_ok: true,
    });
    expect(screen.queryByText("must be ignored")).not.toBeInTheDocument();

    const { unmount: unmountAfterRegistration } = render(
      <StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />,
    );
    await waitFor(() => expect(staleEndedHandler).not.toBeNull());
    vi.mocked(ipc.listStreamingSessions).mockClear();
    unmountAfterRegistration();
    staleEndedHandler?.({ session_id: 1 });

    expect(ipc.listStreamingSessions).not.toHaveBeenCalled();
  });

  it("cleans up every listener that registered before another registration fails", async () => {
    // Registration is independent per event. A failure in the last one must
    // neither leak the four listeners that already resolved nor escape as an
    // unhandled rejected effect promise.
    vi.mocked(ipc.onStreamingPartial).mockRejectedValueOnce(
      new Error("partial listener unavailable"),
    );

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await waitFor(() =>
      expect(ipc.onStreamingPartial).toHaveBeenCalledTimes(1),
    );

    expect(windowHandler).toBeNull();
    expect(sourcesHandler).toBeNull();
    expect(endedHandler).toBeNull();
  });

  it("contains a failed initial session refresh instead of leaking an unhandled rejection", async () => {
    vi.mocked(ipc.listStreamingSessions).mockRejectedValueOnce(
      new Error("initial list unavailable"),
    );

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    // The hydration attempt is fire-and-forget by design. Reaching the
    // workspace after its rejected promise proves its rejection is contained.
    expect(
      await screen.findByRole("button", { name: "Start" }),
    ).toBeInTheDocument();
  });

  it("contains a failed refresh triggered by session-ended", async () => {
    const user = userEvent.setup();
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(endedHandler).not.toBeNull());
    vi.mocked(ipc.listStreamingSessions).mockRejectedValueOnce(
      new Error("ended list unavailable"),
    );

    act(() => endedHandler?.({ session_id: 2 }));

    await waitFor(() =>
      expect(ipc.listStreamingSessions).toHaveBeenCalledTimes(2),
    );
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();
  });

  it("clears the active partial when its streaming session ends", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.openStreamingSession).mockResolvedValue(openedSession());
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));
    await waitFor(() => {
      expect(partialHandler).not.toBeNull();
      expect(endedHandler).not.toBeNull();
    });
    act(() =>
      partialHandler?.({
        session_id: SESSION_A.id,
        item_id: "draft",
        text: "draft that must disappear",
      }),
    );

    act(() => endedHandler?.({ session_id: SESSION_A.id }));

    await waitFor(() =>
      expect(ipc.listStreamingSessions).toHaveBeenCalledTimes(2),
    );
  });

  it("stops sequential event registration when a later listener resolves after unmount", async () => {
    const registration = deferred<() => void>();
    const unlisten = vi.fn();
    vi.mocked(ipc.onStreamingSessionEnded).mockReturnValueOnce(
      registration.promise,
    );

    const { unmount } = render(
      <StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />,
    );
    await waitFor(() =>
      expect(ipc.onStreamingSessionEnded).toHaveBeenCalledOnce(),
    );

    unmount();
    await act(async () => registration.resolve(unlisten));

    expect(unlisten).toHaveBeenCalledOnce();
    expect(ipc.onStreamingPartial).not.toHaveBeenCalled();
  });

  it("contains a synchronous listener-registration failure", async () => {
    vi.mocked(ipc.onStreamingWindow).mockImplementationOnce(() => {
      throw new Error("window listener unavailable");
    });

    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await waitFor(() => expect(ipc.onStreamingWindow).toHaveBeenCalledOnce());
    expect(ipc.onStreamingSources).not.toHaveBeenCalled();
  });

  it("reports mixed mic + system audio sources", async () => {
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
      translation_enabled: false,
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
      translation_enabled: false,
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
      translation_enabled: false,
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
      "meeting 1 was not found",
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));

    expect(
      await screen.findByText(/meeting 1 was not found/),
    ).toBeInTheDocument();
  });

  it("a failed rename surfaces the error", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.renameStreamingSession).mockRejectedValue("rename failed");
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

  it("allows retrying a rejected delete from the still-open confirmation", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.deleteStreamingSession)
      .mockRejectedValueOnce(new Error("delete failed"))
      .mockResolvedValueOnce(undefined);
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await screen.findByText("Standup");

    await user.click(screen.getByRole("button", { name: "Delete Standup" }));
    const dialog = screen.getByRole("alertdialog", { name: "Delete Standup" });
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(await screen.findByText(/delete failed/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));

    await waitFor(() =>
      expect(ipc.deleteStreamingSession).toHaveBeenCalledTimes(2),
    );
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
            text: "mfu to be cleared",
            language: "en",
            outcome_ok: true,
          },
        ],
      }),
    );
    vi.mocked(ipc.deleteStreamingSession).mockResolvedValue(undefined);
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));
    await screen.findByText(/mfu to be cleared/);
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([]);

    await user.click(screen.getByRole("button", { name: "Delete Standup" }));
    const dialog = screen.getByRole("alertdialog", { name: "Delete Standup" });
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));

    expect(screen.queryByText(/mfu to be cleared/)).not.toBeInTheDocument();
    expect(
      await screen.findByText(/Start a meeting, or open one/),
    ).toBeInTheDocument();
  });

  it("Copy and Export are disabled until there is transcript text", async () => {
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await screen.findByText(/Start a meeting, or open one/);

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
        translation_enabled: false,
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
      translation_enabled: false,
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
});
