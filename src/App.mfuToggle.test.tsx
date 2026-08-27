import { mockCreateIpc } from "./test/ipcMock";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import * as ipc from "./ipc";
import {
  TRANSCRIPTION_DOWNLOADED,
  waitForAddFileEnabled,
  chooseAndTranscribe,
  resetAppMocks,
} from "./test/appTestHarness";

vi.mock("./ipc", () => mockCreateIpc());

beforeEach(resetAppMocks);

// WP-90: the view-only MFU switch in the Meeting transcript header. Default
// ON; persists per screen under the `mfu_panel_meeting` setting key; never
// gates Craft MFU/Diarize/Transcribe.
describe("App — MFU panel toggle", () => {
  it("renders checked-on by default, after Diarize, and shows the MFU panel", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    render(<App />);
    await waitForAddFileEnabled();

    const toggle = await screen.findByRole("switch", { name: /mfu/i });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(document.querySelector("aside.wp-mfu")).toBeInTheDocument();

    const actions = toggle.closest(".wp-transcript-actions");
    expect(actions).not.toBeNull();
    const diarize = screen.getByRole("button", { name: "Diarize speakers" });
    const order = Array.from(actions!.querySelectorAll<HTMLElement>("button"));
    expect(order.indexOf(toggle)).toBeGreaterThan(order.indexOf(diarize));
  });

  it("switching MFU off hides aside.wp-mfu and persists the meeting-scoped key", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);
    const toggle = await screen.findByRole("switch", { name: /mfu/i });

    await user.click(toggle);

    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(document.querySelector("aside.wp-mfu")).not.toBeInTheDocument();
    expect(ipc.setSetting).toHaveBeenCalledWith("mfu_panel_meeting", "false");
  });

  it("switching MFU back on restores aside.wp-mfu", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);
    const toggle = await screen.findByRole("switch", { name: /mfu/i });

    await user.click(toggle);
    expect(document.querySelector("aside.wp-mfu")).not.toBeInTheDocument();
    await user.click(toggle);

    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(document.querySelector("aside.wp-mfu")).toBeInTheDocument();
    expect(ipc.setSetting).toHaveBeenLastCalledWith(
      "mfu_panel_meeting",
      "true",
    );
  });

  it("restores a hidden panel on launch when the persisted setting is off", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
      mfu_panel_meeting: false,
      mfu_panel_streaming: true,
    });
    render(<App />);

    const toggle = await screen.findByRole("switch", { name: /mfu/i });
    await waitFor(() =>
      expect(toggle).toHaveAttribute("aria-checked", "false"),
    );
    expect(document.querySelector("aside.wp-mfu")).not.toBeInTheDocument();
  });

  it("falls back to the ON default, without a blocking error, when settings cannot be read", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.getSettings).mockRejectedValue(new Error("disk unavailable"));
    render(<App />);

    const toggle = await screen.findByRole("switch", { name: /mfu/i });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(document.querySelector("aside.wp-mfu")).toBeInTheDocument();
    // No blocking error is surfaced for the MFU toggle's own settings read —
    // the screen keeps rendering normally.
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Choose file" }),
    ).toBeInTheDocument();
  });

  it("keeps the screen usable, with no blocking error, when persisting the toggle fails", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.setSetting).mockRejectedValue(new Error("disk full"));
    const user = userEvent.setup();
    render(<App />);
    const toggle = await screen.findByRole("switch", { name: /mfu/i });

    await user.click(toggle);

    await waitFor(() =>
      expect(document.querySelector("aside.wp-mfu")).not.toBeInTheDocument(),
    );
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("running Craft MFU while the panel is hidden reveals it and leaves the switch on, without cancelling generation", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([
      TRANSCRIPTION_DOWNLOADED,
      {
        id: "llm-1",
        task: "llm",
        label: "Local LLM",
        downloaded: true,
        size_bytes: 1,
        recommended: false,
      },
    ]);
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      active_model_llm: "llm-1",
      export_file_type: "plain_text",
    });
    const user = userEvent.setup();
    let resolveGenerate!: (meeting: ipc.Meeting) => void;
    vi.mocked(ipc.generateMfu).mockReturnValue(
      new Promise((resolve) => {
        resolveGenerate = resolve;
      }),
    );
    render(<App />);
    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hello");

    const toggle = await screen.findByRole("switch", { name: /mfu/i });
    await user.click(toggle);
    expect(document.querySelector("aside.wp-mfu")).not.toBeInTheDocument();

    const craft = await screen.findByRole("button", { name: "Craft MFU" });
    await waitFor(() => expect(craft).not.toBeDisabled());
    await user.click(craft);

    expect(document.querySelector("aside.wp-mfu")).toBeInTheDocument();
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(ipc.generateMfu).toHaveBeenCalledTimes(1);

    resolveGenerate({
      id: 100,
      title: "New Meeting",
      created_at_ms: 0,
      language: "ru",
      status: "finished",
      segments: [],
      source_missing: false,
      mfu: {
        meeting_id: 100,
        summary: "Summary text",
        decisions: "",
        action_items: "",
        open_questions: "",
        participants: "",
      },
    });

    // The completed result is not left behind a hidden panel.
    expect(await screen.findByDisplayValue("Summary text")).toBeInTheDocument();
    expect(document.querySelector("aside.wp-mfu")).toBeInTheDocument();
  });

  it("does not change Diarize's, Craft MFU's, or Transcribe's enabled/disabled behavior when the switch is toggled", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);
    const toggle = await screen.findByRole("switch", { name: /mfu/i });
    const diarize = screen.getByRole("button", { name: "Diarize speakers" });
    const craft = screen.getByRole("button", { name: "Craft MFU" });
    const transcribe = screen.getByRole("button", { name: "Transcribe" });
    const diarizeWasDisabled = diarize.hasAttribute("disabled");
    const craftWasDisabled = craft.hasAttribute("disabled");
    const transcribeWasDisabled = transcribe.hasAttribute("disabled");

    await user.click(toggle);

    expect(diarize.hasAttribute("disabled")).toBe(diarizeWasDisabled);
    expect(craft.hasAttribute("disabled")).toBe(craftWasDisabled);
    expect(transcribe.hasAttribute("disabled")).toBe(transcribeWasDisabled);
  });

  it("switching between Meeting and Streaming keeps each screen's MFU state independent (per-screen persistence)", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    // Simulates state already persisted from a prior session: Meeting hidden,
    // Streaming shown.
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
      mfu_panel_meeting: false,
      mfu_panel_streaming: true,
    });
    const user = userEvent.setup();
    render(<App />);
    await waitForAddFileEnabled();

    const meetingToggle = await screen.findByRole("switch", { name: /mfu/i });
    await waitFor(() =>
      expect(meetingToggle).toHaveAttribute("aria-checked", "false"),
    );

    await user.click(screen.getByRole("button", { name: "Streaming" }));

    const streamingToggle = await screen.findByRole("switch", {
      name: /mfu/i,
    });
    expect(streamingToggle).toHaveAttribute("aria-checked", "true");
    expect(document.querySelector("aside.wp-mfu")).toBeInTheDocument();
  });
});
