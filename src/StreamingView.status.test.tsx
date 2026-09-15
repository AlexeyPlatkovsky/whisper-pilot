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
void [
  fireEvent,
  within,
  readCssBundle,
  windowHandler,
  sourcesHandler,
  partialHandler,
  writeTextMock,
  deferred,
  MFU,
  ONE_WINDOW,
];

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

describe("StreamingView — status", () => {
  describe("status widget", () => {
    // S-2: first-launch / empty state
    it("shows Ready with no timer before any session has run", async () => {
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await screen.findByText(/Start a meeting, or open one/);

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
        translation_enabled: boolean;
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
      liveCaptureRevision += 1;
      act(() => {
        liveCaptureHandler!({
          phase: "starting",
          session_id: 2,
          source: "streaming",
          generation: 1,
          revision: liveCaptureRevision,
          error: null,
        });
      });
      expect(status).toHaveTextContent("Starting…");

      resolveStart({
        id: 2,
        title: "New Streaming Session",
        created_at_ms: 200,
        updated_at_ms: 200,
        status: "active",
        translation_enabled: false,
      });
      liveCaptureRevision += 1;
      liveCaptureHandler!({
        phase: "capturing",
        session_id: 2,
        source: "streaming",
        generation: 1,
        revision: liveCaptureRevision,
        error: null,
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
          translation_enabled: false,
        });
        render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
        const status = await screen.findByRole("status");

        await user.click(await screen.findByRole("button", { name: "Start" }));
        await waitFor(() => expect(status).toHaveTextContent("On Air"));

        // The advance fires one interval callback per simulated second, and
        // firing thousands of them takes real time — seconds under coverage
        // instrumentation — which shouldAdvanceTime bleeds into the measured
        // elapsed. Assert bleed-tolerant windows on each side of the 60
        // minute format switch instead of exact boundary seconds. The
        // generous timeout absorbs the thousands of instrumented re-renders.
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
    }, 60_000);

    // S-5: the session ending on its own (not via manual Stop) still resets the widget
    it("returns to Ready when the session ends on its own, not only via manual Stop", async () => {
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 2,
        title: "New Streaming Session",
        created_at_ms: 200,
        updated_at_ms: 200,
        status: "active",
        translation_enabled: false,
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

      await user.click(
        await screen.findByRole("button", { name: "Open Standup" }),
      );

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
        translation_enabled: false,
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
});
