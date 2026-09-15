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
  endedHandler,
  partialHandler,
  writeTextMock,
  MFU,
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

describe("StreamingView — cloud", () => {
  describe("Cloud engine selector", () => {
    // WP-106 scenario/C-4, state-transition: Cloud shows the required notice
    // on every selection and Local clears it without changing audio state.
    it("defaults to Local and repeats the cloud audio/billing notice on each Cloud selection", async () => {
      const user = userEvent.setup();
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      const local = await screen.findByRole("button", {
        name: "Use local transcription",
      });
      const cloud = screen.getByRole("button", {
        name: "Use cloud transcription",
      });
      expect(local).toHaveAttribute("aria-pressed", "true");
      expect(cloud).toHaveAttribute("aria-pressed", "false");

      await user.click(cloud);
      expect(
        await screen.findByText(
          "Cloud transcription sends live audio to Deepgram. Usage is billed to your account.",
        ),
      ).toBeInTheDocument();

      await user.click(local);
      expect(
        screen.queryByText(
          "Cloud transcription sends live audio to Deepgram. Usage is billed to your account.",
        ),
      ).not.toBeInTheDocument();

      await user.click(cloud);
      expect(
        await screen.findByText(
          "Cloud transcription sends live audio to Deepgram. Usage is billed to your account.",
        ),
      ).toBeInTheDocument();
    });

    it("starts a configured Cloud session without falling back to Local", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 77,
        title: "Cloud session",
        created_at_ms: 1,
        updated_at_ms: 2,
        status: "active",
        translation_enabled: false,
      });
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(
        await screen.findByRole("button", { name: "Use cloud transcription" }),
      );
      await user.click(screen.getByRole("button", { name: "Start" }));

      await waitFor(() =>
        expect(ipc.startStreamingSession).toHaveBeenCalledWith(
          undefined,
          "cloud",
        ),
      );
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(screen.getByText("Cloud session")).toBeInTheDocument();
    });

    it("starts a new Local session instead of resuming a stopped Cloud session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({
          windows: ONE_WINDOW,
          transcription_engine: "cloud",
        }),
      );
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        ...SESSION_A,
        id: 2,
        status: "active",
      });
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(await screen.findByText("Standup"));
      await user.click(
        screen.getByRole("button", { name: "Use local transcription" }),
      );
      await user.click(await screen.findByRole("button", { name: "Resume" }));

      await waitFor(() =>
        expect(ipc.startStreamingSession).toHaveBeenCalledWith(undefined),
      );
    });

    // WP-106 C-4, EP: an unconfigured provider must explain the prerequisite
    // and never begin Local capture as a fallback.
    it("blocks an unconfigured Cloud start before it can start capture", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.getCloudProviderConfig).mockResolvedValue({
        selected_provider: "assemblyai",
        providers: [
          {
            id: "deepgram",
            name: "Deepgram",
            model: "Nova-3",
            configured: false,
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
            configured: false,
          },
        ],
      });
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(
        await screen.findByRole("button", { name: "Use cloud transcription" }),
      );
      await user.click(screen.getByRole("button", { name: "Start" }));

      expect(await screen.findByRole("alert")).toHaveTextContent(
        "Configure a AssemblyAI API key in Settings before starting Cloud transcription.",
      );
      expect(ipc.startStreamingSession).not.toHaveBeenCalled();
    });

    // WP-106 C-4, state transition: closing Settings refreshes the mounted
    // Streaming view so its disclosure and start guard use the selected card.
    it("refreshes the Cloud provider after Settings closes", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.getCloudProviderConfig)
        .mockResolvedValueOnce({
          selected_provider: "deepgram",
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
              configured: false,
            },
          ],
        })
        .mockResolvedValueOnce({
          selected_provider: "openai",
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
        });
      const view = render(
        <StreamingView
          onClose={vi.fn()}
          onOpenSettings={vi.fn()}
          settingsOpen={false}
        />,
      );
      await user.click(
        await screen.findByRole("button", { name: "Use cloud transcription" }),
      );
      expect(
        await screen.findByText(/sends live audio to Deepgram/i),
      ).toBeInTheDocument();

      view.rerender(
        <StreamingView
          onClose={vi.fn()}
          onOpenSettings={vi.fn()}
          settingsOpen
        />,
      );
      view.rerender(
        <StreamingView
          onClose={vi.fn()}
          onOpenSettings={vi.fn()}
          settingsOpen={false}
        />,
      );

      expect(
        await screen.findByText(/sends live audio to OpenAI/i),
      ).toBeInTheDocument();
    });

    // WP-106 C-4, concurrency boundary: a late mount request must not undo
    // a newer configuration refresh triggered when Settings closes.
    it("keeps the newest Cloud configuration when refreshes resolve out of order", async () => {
      const user = userEvent.setup();
      const first = deferred<CloudProviderConfiguration>();
      const second = deferred<CloudProviderConfiguration>();
      vi.mocked(ipc.getCloudProviderConfig)
        .mockReturnValueOnce(first.promise)
        .mockReturnValueOnce(second.promise);
      const view = render(
        <StreamingView
          onClose={vi.fn()}
          onOpenSettings={vi.fn()}
          settingsOpen={false}
        />,
      );

      view.rerender(
        <StreamingView
          onClose={vi.fn()}
          onOpenSettings={vi.fn()}
          settingsOpen
        />,
      );
      view.rerender(
        <StreamingView
          onClose={vi.fn()}
          onOpenSettings={vi.fn()}
          settingsOpen={false}
        />,
      );
      await act(async () => {
        second.resolve(cloudProviderConfiguration("openai"));
      });
      await user.click(
        await screen.findByRole("button", { name: "Use cloud transcription" }),
      );
      expect(
        await screen.findByText(/sends live audio to OpenAI/i),
      ).toBeInTheDocument();

      await act(async () => {
        first.resolve(cloudProviderConfiguration("deepgram"));
      });
      expect(
        screen.getByText(/sends live audio to OpenAI/i),
      ).toBeInTheDocument();
    });

    // WP-106 C-4, state transition: the mode cannot change while the Local
    // start request is in flight, before the active session event arrives.
    it("locks engine selection while a Local start request is pending", async () => {
      const user = userEvent.setup();
      const start = deferred<StreamingSessionSummary>();
      vi.mocked(ipc.startStreamingSession).mockReturnValue(start.promise);
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(await screen.findByRole("button", { name: "Start" }));
      await waitFor(() =>
        expect(ipc.startStreamingSession).toHaveBeenCalledTimes(1),
      );
      expect(
        screen.getByRole("button", { name: "Use local transcription" }),
      ).toBeDisabled();
      expect(
        screen.getByRole("button", { name: "Use cloud transcription" }),
      ).toBeDisabled();

      await act(async () => {
        start.resolve({
          id: 2,
          title: "New Streaming Session",
          created_at_ms: 200,
          updated_at_ms: 200,
          status: "active",
          translation_enabled: false,
        });
      });
    });

    // WP-106 scenario/C-4, state-transition: once capture begins, the whole
    // transcript header becomes unavailable rather than showing a Locked badge.
    it("disables the transcript-header engine, language, and action controls while streaming", async () => {
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
        await screen.findByRole("button", { name: "Use local transcription" }),
      ).toBeDisabled();
      expect(
        screen.getByRole("button", { name: "Use cloud transcription" }),
      ).toBeDisabled();
      expect(
        screen.getByRole("switch", { name: "Live Translation" }),
      ).toBeDisabled();
      expect(
        screen.getByRole("button", { name: "Prettify transcript" }),
      ).toBeDisabled();
      expect(screen.queryByText("Locked")).not.toBeInTheDocument();
    });
  });
});
