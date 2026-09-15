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
  act,
  fireEvent,
  readCssBundle,
  sourcesHandler,
  endedHandler,
  partialHandler,
  writeTextMock,
  deferred,
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

describe("StreamingView — mfu", () => {
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

      expect(screen.getByRole("button", { name: "Craft MFU" })).toBeDisabled();

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
        await screen.findByRole("button", { name: "Craft MFU" }),
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
        await screen.findByRole("button", { name: "Craft MFU" }),
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
        translation_enabled: false,
      });
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByRole("button", { name: "Start" }));
      await waitFor(() => expect(windowHandler).not.toBeNull());
      windowHandler!({ ...ONE_WINDOW[0], session_id: 2 });

      expect(
        await screen.findByRole("button", { name: "Craft MFU" }),
      ).toBeDisabled();
    });

    // S-1: happy path
    it("shows the placeholder before Craft, then the generated mfu after", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      let resolveCraft!: (v: StreamingSession) => void;
      vi.mocked(ipc.generateStreamingMfu).mockReturnValue(
        new Promise((resolve) => {
          resolveCraft = resolve;
        }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      expect(screen.getByText("Run MFU Craft")).toBeInTheDocument();
      const status = await screen.findByRole("status");

      await user.click(
        await screen.findByRole("button", { name: "Craft MFU" }),
      );

      expect(status).toHaveTextContent("Crafting MFU");
      expect(ipc.generateStreamingMfu).toHaveBeenCalledWith(1);

      resolveCraft(openedSession({ windows: ONE_WINDOW, mfu: MFU }));

      expect(
        await screen.findByText("Discussed Q3 roadmap."),
      ).toBeInTheDocument();
      expect(screen.getByText("Ship M1 by Friday.")).toBeInTheDocument();
      expect(screen.queryByText("Run MFU Craft")).not.toBeInTheDocument();
      await waitFor(() => expect(status).toHaveTextContent("Ready"));
    });

    it("ticks an elapsed timer once per second while Crafting MFU", async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      try {
        const user = userEvent.setup({
          advanceTimers: vi.advanceTimersByTime.bind(vi),
        });
        vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
        vi.mocked(ipc.openStreamingSession).mockResolvedValue(
          openedSession({ windows: ONE_WINDOW }),
        );
        vi.mocked(ipc.generateStreamingMfu).mockReturnValue(
          new Promise(() => {
            // Never resolves — only the ticking timer is under test here.
          }),
        );
        render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
        await user.click(await screen.findByText("Standup"));
        const status = await screen.findByRole("status");

        await user.click(
          await screen.findByRole("button", { name: "Craft MFU" }),
        );
        expect(status.querySelector(".wp-status-timer")?.textContent).toBe(
          "00:00",
        );

        // Same bleed consideration as the h:mm:ss test above: real wall-clock
        // folded into the fake clock can add a second or two on a slow,
        // instrumented runner, so assert a small window rather than an exact
        // boundary.
        await vi.advanceTimersByTimeAsync(3000);

        expect(status.querySelector(".wp-status-timer")?.textContent).toMatch(
          /^00:0[3-9]$/,
        );
      } finally {
        vi.useRealTimers();
      }
    }, 60_000);

    // Empty mfu sections are omitted, matching Meeting's rendering
    it("omits empty mfu sections", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingMfu).mockResolvedValue(
        openedSession({
          windows: ONE_WINDOW,
          mfu: { ...MFU, participants: "" },
        }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(
        await screen.findByRole("button", { name: "Craft MFU" }),
      );

      await screen.findByText("Discussed Q3 roadmap.");
      expect(screen.queryByText("Participants")).not.toBeInTheDocument();
    });

    // S-7: mfu persist across reopen
    it("shows previously generated mfu when reopening a session", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW, mfu: MFU }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(await screen.findByText("Standup"));

      expect(
        await screen.findByText("Discussed Q3 roadmap."),
      ).toBeInTheDocument();
      expect(ipc.generateStreamingMfu).not.toHaveBeenCalled();
    });

    // S-2: failure surfaces in the error banner and the widget
    it("shows the error banner and MFU Failed on a Craft failure", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
      vi.mocked(ipc.openStreamingSession).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW }),
      );
      vi.mocked(ipc.generateStreamingMfu).mockRejectedValue(
        "no LLM model selected in Settings",
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");

      await user.click(
        await screen.findByRole("button", { name: "Craft MFU" }),
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
      vi.mocked(ipc.generateStreamingMfu).mockRejectedValueOnce("failed");
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU" }),
      );
      await waitFor(() => expect(status).toHaveTextContent("MFU Failed"));

      // Time passing alone does not clear it — no auto-revert timer.
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(status).toHaveTextContent("MFU Failed");

      vi.mocked(ipc.generateStreamingMfu).mockResolvedValue(
        openedSession({ windows: ONE_WINDOW, mfu: MFU }),
      );
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU" }),
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
      vi.mocked(ipc.generateStreamingMfu).mockRejectedValue("failed");
      vi.mocked(ipc.startStreamingSession).mockResolvedValue({
        id: 1,
        title: "Standup",
        created_at_ms: 100,
        updated_at_ms: 200,
        status: "active",
        translation_enabled: false,
      });
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU" }),
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
      vi.mocked(ipc.generateStreamingMfu).mockRejectedValue("failed");
      vi.mocked(ipc.deleteStreamingSession).mockResolvedValue(undefined);
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const status = await screen.findByRole("status");
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU" }),
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
      vi.mocked(ipc.generateStreamingMfu).mockReturnValue(
        new Promise((resolve) => {
          resolveCraft = resolve;
        }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      const craftButton = await screen.findByRole("button", {
        name: "Craft MFU",
      });

      await user.click(craftButton);
      expect(craftButton).toBeDisabled();

      resolveCraft(openedSession({ windows: ONE_WINDOW, mfu: MFU }));
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
      vi.mocked(ipc.generateStreamingMfu).mockReturnValue(
        new Promise((resolve) => {
          resolveCraft = resolve;
        }),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU" }),
      );

      await user.click(await screen.findByText("Design Review"));
      resolveCraft(
        openedSession({
          id: 1,
          title: "Standup",
          windows: ONE_WINDOW,
          mfu: MFU,
        }),
      );

      expect(
        screen.queryByText("Discussed Q3 roadmap."),
      ).not.toBeInTheDocument();
      expect(screen.getByText("Run MFU Craft")).toBeInTheDocument();
    });

    it("keeps a Craft failure owned by its session after navigating away", async () => {
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
      vi.mocked(ipc.generateStreamingMfu).mockRejectedValue("LLM unavailable");
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU" }),
      );
      await waitFor(() =>
        expect(screen.getByRole("status")).toHaveTextContent("MFU Failed"),
      );

      await user.click(await screen.findByText("Design Review"));
      expect(screen.getByRole("status")).toHaveTextContent("Ready");

      await user.click(await screen.findByText("Standup"));
      await waitFor(() =>
        expect(screen.getByRole("status")).toHaveTextContent("MFU Failed"),
      );
    });

    it("keeps Craft and Prettify single-flight after switching away from the crafting session", async () => {
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
      vi.mocked(ipc.generateStreamingMfu).mockReturnValue(
        new Promise(() => {}),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Craft MFU" }),
      );
      await user.click(await screen.findByText("Design Review"));

      // The result remains owned by Standup, but the shared local LLM must
      // never accept a second Craft or Prettify while that request runs.
      expect(screen.getByRole("button", { name: "Craft MFU" })).toBeDisabled();
      expect(
        screen.getByRole("button", { name: "Prettify transcript" }),
      ).toBeDisabled();
    });
  });
});
