import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StreamingView } from "./StreamingView";
import * as ipc from "./ipc";
import type {
  StreamingMfu,
  StreamingSession,
  StreamingSessionSummary,
} from "./ipc";

// WP-90: the view-only MFU switch in the Streaming transcript header, mirroring
// App.mfuToggle.test.tsx for the Meeting screen. Default ON; persists under
// the `mfu_panel_streaming` key; never gates Craft MFU/Prettify/Start/Stop.
vi.mock("./ipc", () => ({
  listStreamingSessions: vi.fn(async () => []),
  openStreamingSession: vi.fn(),
  renameStreamingSession: vi.fn(),
  deleteStreamingSession: vi.fn(),
  createStreamingSession: vi.fn(),
  startStreamingSession: vi.fn(),
  stopStreamingSession: vi.fn(),
  generateStreamingMfu: vi.fn(),
  generateStreamingPrettify: vi.fn(),
  acceptStreamingPrettify: vi.fn(),
  revertStreamingPrettify: vi.fn(),
  translateStreamingParagraph: vi.fn(),
  listStreamingTranslations: vi.fn(async () => []),
  onStreamingWindow: vi.fn(async () => () => {}),
  onStreamingSources: vi.fn(async () => () => {}),
  onStreamingSessionEnded: vi.fn(async () => () => {}),
  saveTextDialog: vi.fn(async () => null),
  getSettings: vi.fn(async () => ({
    theme: "system",
    ui_language: "en",
    active_model_diarization: "none",
    export_file_type: "plain_text",
  })),
  setSetting: vi.fn(),
  listTaskModels: vi.fn(async () => []),
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

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(ipc.listStreamingSessions).mockResolvedValue([]);
  vi.mocked(ipc.getSettings).mockResolvedValue({
    theme: "system",
    ui_language: "en",
    active_model_diarization: "none",
    export_file_type: "plain_text",
  });
});

describe("StreamingView — MFU panel toggle", () => {
  it("renders checked-on by default, after Prettify, and shows the MFU panel", async () => {
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: /mfu/i });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(document.querySelector("aside.wp-mfu")).toBeInTheDocument();

    const actions = toggle.closest(".wp-transcript-actions");
    expect(actions).not.toBeNull();
    const prettify = screen.getByRole("button", {
      name: "Prettify transcript",
    });
    const order = Array.from(actions!.querySelectorAll<HTMLElement>("button"));
    expect(order.indexOf(toggle)).toBeGreaterThan(order.indexOf(prettify));
  });

  it("switching MFU off hides aside.wp-mfu and persists the streaming-scoped key", async () => {
    const user = userEvent.setup();
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    const toggle = await screen.findByRole("switch", { name: /mfu/i });

    await user.click(toggle);

    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(document.querySelector("aside.wp-mfu")).not.toBeInTheDocument();
    expect(ipc.setSetting).toHaveBeenCalledWith("mfu_panel_streaming", "false");
  });

  it("switching MFU back on restores aside.wp-mfu", async () => {
    const user = userEvent.setup();
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    const toggle = await screen.findByRole("switch", { name: /mfu/i });

    await user.click(toggle);
    expect(document.querySelector("aside.wp-mfu")).not.toBeInTheDocument();
    await user.click(toggle);

    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(document.querySelector("aside.wp-mfu")).toBeInTheDocument();
    expect(ipc.setSetting).toHaveBeenLastCalledWith(
      "mfu_panel_streaming",
      "true",
    );
  });

  it("restores a hidden panel on launch when the persisted setting is off", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
      mfu_panel_meeting: true,
      mfu_panel_streaming: false,
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: /mfu/i });
    await waitFor(() =>
      expect(toggle).toHaveAttribute("aria-checked", "false"),
    );
    expect(document.querySelector("aside.wp-mfu")).not.toBeInTheDocument();
  });

  it("falls back to the ON default, without a blocking error, when settings cannot be read", async () => {
    vi.mocked(ipc.getSettings).mockRejectedValue(new Error("disk unavailable"));
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: /mfu/i });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(document.querySelector("aside.wp-mfu")).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(
      await screen.findByText("Start a session, or open one from the list."),
    ).toBeInTheDocument();
  });

  it("keeps the screen usable, with no blocking error, when persisting the toggle fails", async () => {
    vi.mocked(ipc.setSetting).mockRejectedValue(new Error("disk full"));
    const user = userEvent.setup();
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    const toggle = await screen.findByRole("switch", { name: /mfu/i });

    await user.click(toggle);

    await waitFor(() =>
      expect(document.querySelector("aside.wp-mfu")).not.toBeInTheDocument(),
    );
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("running Craft MFU while the panel is hidden reveals it and leaves the switch on, without cancelling generation", async () => {
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

    const toggle = await screen.findByRole("switch", { name: /mfu/i });
    await user.click(toggle);
    expect(document.querySelector("aside.wp-mfu")).not.toBeInTheDocument();

    const craft = await screen.findByRole("button", { name: "Craft MFU" });
    await waitFor(() => expect(craft).not.toBeDisabled());
    await user.click(craft);

    expect(document.querySelector("aside.wp-mfu")).toBeInTheDocument();
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(ipc.generateStreamingMfu).toHaveBeenCalledTimes(1);

    resolveCraft(openedSession({ windows: ONE_WINDOW, mfu: MFU }));

    expect(
      await screen.findByText("Discussed Q3 roadmap."),
    ).toBeInTheDocument();
    expect(document.querySelector("aside.wp-mfu")).toBeInTheDocument();
  });

  it("does not change Prettify's, Craft MFU's, Start's, or Stop's enabled/disabled behavior when the switch is toggled", async () => {
    const user = userEvent.setup();
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    const toggle = await screen.findByRole("switch", { name: /mfu/i });
    const prettify = screen.getByRole("button", {
      name: "Prettify transcript",
    });
    const craft = screen.getByRole("button", { name: "Craft MFU" });
    const start = screen.getByRole("button", { name: "Start" });
    const stop = screen.getByRole("button", { name: "Stop" });
    const prettifyWasDisabled = prettify.hasAttribute("disabled");
    const craftWasDisabled = craft.hasAttribute("disabled");
    const startWasDisabled = start.hasAttribute("disabled");
    const stopWasDisabled = stop.hasAttribute("disabled");

    await user.click(toggle);

    expect(prettify.hasAttribute("disabled")).toBe(prettifyWasDisabled);
    expect(craft.hasAttribute("disabled")).toBe(craftWasDisabled);
    expect(start.hasAttribute("disabled")).toBe(startWasDisabled);
    expect(stop.hasAttribute("disabled")).toBe(stopWasDisabled);
  });
});
