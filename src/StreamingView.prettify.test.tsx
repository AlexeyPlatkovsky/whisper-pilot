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
  deferred,
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

describe("StreamingView — prettify", () => {
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
        translation_enabled: false,
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
      vi.mocked(ipc.generateStreamingMfu).mockReturnValue(
        new Promise(() => {}),
      );
      vi.mocked(ipc.generateStreamingPrettify).mockReturnValue(
        new Promise(() => {}),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
      await user.click(await screen.findByText("Standup"));

      await user.click(
        await screen.findByRole("button", { name: "Craft MFU" }),
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
        await screen.findByRole("button", { name: "Craft MFU" }),
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
        translation_enabled: false,
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

    it("keeps Craft and Prettify single-flight after switching away from the prettifying session", async () => {
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
      vi.mocked(ipc.generateStreamingPrettify).mockReturnValue(
        new Promise(() => {}),
      );
      render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

      await user.click(await screen.findByText("Standup"));
      await user.click(
        await screen.findByRole("button", { name: "Prettify transcript" }),
      );
      await user.click(await screen.findByText("Design Review"));

      expect(screen.getByRole("button", { name: "Craft MFU" })).toBeDisabled();
      expect(
        screen.getByRole("button", { name: "Prettify transcript" }),
      ).toBeDisabled();
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
