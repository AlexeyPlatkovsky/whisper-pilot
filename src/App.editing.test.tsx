import { mockCreateIpc } from "./test/ipcMock";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import * as ipc from "./ipc";
import {
  TRANSCRIPTION_DOWNLOADED,
  HELLO_SEGMENT,
  transcribedMeeting,
  transcribeResult,
  waitForAddFileEnabled,
  chooseAndTranscribe,
  resetAppMocks,
} from "./test/appTestHarness";

vi.mock("./ipc", () => mockCreateIpc());

beforeEach(resetAppMocks);

const DIARIZATION_DOWNLOADED = {
  id: "diarization-campplus",
  task: "diarization",
  label: "CAM++",
  downloaded: true,
  size_bytes: 1,
  recommended: false,
};

describe("App — transcript editing", () => {
  it("updates a segment when the user types in its textarea", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    const textarea = await screen.findByDisplayValue("Hello");

    await user.clear(textarea);
    await user.type(textarea, "Hi there");

    expect(screen.getByDisplayValue("Hi there")).toBeInTheDocument();

    // Flush the real 500ms auto-save timer before the test ends, so it cannot
    // fire mid-test later and pollute another test's updateSegment call count.
    await waitFor(
      () => expect(ipc.updateSegment).toHaveBeenCalledWith(100, 0, "Hi there"),
      { timeout: 4000 },
    );
  });

  it("auto-saves an edited segment after a debounce, with no explicit save action", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = userEvent.setup({
        advanceTimers: vi.advanceTimersByTime.bind(vi),
      });
      render(<App />);
      await waitForAddFileEnabled();
      await chooseAndTranscribe(user);
      const textarea = await screen.findByDisplayValue("Hello");

      await user.clear(textarea);
      await user.type(textarea, "Hi there");
      expect(ipc.updateSegment).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(500);
      expect(ipc.updateSegment).toHaveBeenCalledWith(100, 0, "Hi there");
      expect(ipc.updateSegment).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("debounces rapid keystrokes into a single auto-save call", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);
    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    const textarea = await screen.findByDisplayValue("Hello");

    // Real timers, not fake: both keystrokes land synchronously inside the
    // 500ms debounce window, so the first timer is always cleared before it
    // can fire. Fake timers with shouldAdvanceTime fold real wall-clock into
    // the fake clock, which on a slow runner lets the first timer fire before
    // the second change reschedules it.
    fireEvent.change(textarea, { target: { value: "Hello!" } });
    fireEvent.change(textarea, { target: { value: "Hello!!" } });

    await waitFor(
      () => {
        expect(ipc.updateSegment).toHaveBeenCalledTimes(1);
        expect(ipc.updateSegment).toHaveBeenCalledWith(100, 0, "Hello!!");
      },
      { timeout: 4000 },
    );
  });

  it("surfaces an auto-save failure without discarding the edit", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.updateSegment).mockRejectedValue(new Error("disk full"));
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = userEvent.setup({
        advanceTimers: vi.advanceTimersByTime.bind(vi),
      });
      render(<App />);
      await waitForAddFileEnabled();
      await chooseAndTranscribe(user);
      const textarea = await screen.findByDisplayValue("Hello");

      await user.clear(textarea);
      await user.type(textarea, "Hi there");
      await vi.advanceTimersByTimeAsync(500);

      await screen.findByText(/disk full/);
      expect(screen.getByDisplayValue("Hi there")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("App — diarize speakers", () => {
  async function transcribeWithDiarizationActive(
    user: ReturnType<typeof userEvent.setup>,
  ) {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([
      TRANSCRIPTION_DOWNLOADED,
      DIARIZATION_DOWNLOADED,
    ]);
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "campplus",
      export_file_type: "plain_text",
    });
    render(<App />);
    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hello");
    return screen.findByRole("button", { name: "Diarize speakers" });
  }

  it("is disabled when no diarization model is active", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);
    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hello");

    expect(
      screen.getByRole("button", { name: "Diarize speakers" }),
    ).toBeDisabled();
  });

  it("calls diarizeMeeting and applies the returned speaker ids once clicked", async () => {
    const user = userEvent.setup();
    const diarizeButton = await transcribeWithDiarizationActive(user);
    await waitFor(() => expect(diarizeButton).not.toBeDisabled());
    vi.mocked(ipc.diarizeMeeting).mockResolvedValue({
      meeting: transcribedMeeting([{ ...HELLO_SEGMENT, speaker_id: 1 }]),
      diarization_warning: undefined,
    });

    await user.click(diarizeButton);

    expect(ipc.diarizeMeeting).toHaveBeenCalledWith(100);
    await screen.findByText("Speaker 2");
  });

  it("surfaces a diarization failure as an error without discarding the transcript", async () => {
    const user = userEvent.setup();
    const diarizeButton = await transcribeWithDiarizationActive(user);
    await waitFor(() => expect(diarizeButton).not.toBeDisabled());
    vi.mocked(ipc.diarizeMeeting).mockRejectedValue(
      new Error("no diarization model is active"),
    );

    await user.click(diarizeButton);

    await screen.findByText(/no diarization model is active/);
    expect(screen.getByDisplayValue("Hello")).toBeInTheDocument();
  });

  it("disables Diarize while transcribing, generating notes, or itself running", async () => {
    const user = userEvent.setup();
    const diarizeButton = await transcribeWithDiarizationActive(user);
    await waitFor(() => expect(diarizeButton).not.toBeDisabled());

    vi.mocked(ipc.diarizeMeeting).mockReturnValue(new Promise(() => {}));
    await user.click(diarizeButton);

    expect(diarizeButton).toBeDisabled();
    expect(screen.getByRole("button", { name: "Transcribe" })).toBeDisabled();
  });
});

describe("App — notes editing", () => {
  async function transcribeAndCraftNotes(
    user: ReturnType<typeof userEvent.setup>,
  ) {
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
    vi.mocked(ipc.generateNotes).mockResolvedValue({
      ...transcribedMeeting([HELLO_SEGMENT]),
      notes: {
        meeting_id: 100,
        summary: "Summary text",
        decisions: "",
        action_items: "",
        open_questions: "",
        participants: "",
      },
    });

    render(<App />);
    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    const craft = await screen.findByRole("button", { name: "Craft notes" });
    await waitFor(() => expect(craft).not.toBeDisabled());
    await user.click(craft);
    return screen.findByDisplayValue("Summary text");
  }

  it("auto-saves an edited notes field after a debounce", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = userEvent.setup({
        advanceTimers: vi.advanceTimersByTime.bind(vi),
      });
      const summaryField = await transcribeAndCraftNotes(user);

      await user.clear(summaryField);
      await user.type(summaryField, "Updated summary");
      expect(ipc.updateNotes).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(500);

      expect(ipc.updateNotes).toHaveBeenCalledTimes(1);
      expect(ipc.updateNotes).toHaveBeenCalledWith(
        expect.objectContaining({
          meeting_id: 100,
          summary: "Updated summary",
        }),
      );
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("App — speaker rendering", () => {
  it("labels segments by their real speaker_id, not by array position", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    // Indices 0 and 1 share speaker_id 5 (must render the SAME label,
    // "Speaker 6"); index 2 is speaker_id 2 ("Speaker 3"). An index-based
    // fake (e.g. i % N) would instead print three different labels here,
    // so this fails against position-driven labeling, not just no labeling.
    vi.mocked(ipc.transcribeMeeting).mockResolvedValue(
      transcribeResult(
        transcribedMeeting([
          { start_ms: 0, end_ms: 1000, text: "A", speaker_id: 5 },
          { start_ms: 1000, end_ms: 2000, text: "B", speaker_id: 5 },
          { start_ms: 2000, end_ms: 3000, text: "C", speaker_id: 2 },
        ]),
      ),
    );
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);

    await screen.findByDisplayValue("A");
    const [firstSpeaker6, secondSpeaker6] = screen.getAllByText("Speaker 6");
    expect(firstSpeaker6).toBeInTheDocument();
    expect(
      firstSpeaker6
        .closest(".wp-speaker-block")
        ?.querySelector(".wp-speaker-bar"),
    ).toHaveClass("wp-speaker-color-5");
    expect(
      secondSpeaker6
        .closest(".wp-speaker-block")
        ?.querySelector(".wp-speaker-bar"),
    ).toHaveClass("wp-speaker-color-5");

    const speaker3Label = screen.getByText("Speaker 3");
    expect(
      speaker3Label
        .closest(".wp-speaker-block")
        ?.querySelector(".wp-speaker-bar"),
    ).toHaveClass("wp-speaker-color-2");
  });

  it("renders no speaker label when segments carry no speaker_id, and stays editable", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.transcribeMeeting).mockResolvedValue(
      transcribeResult(
        transcribedMeeting([{ start_ms: 0, end_ms: 1000, text: "Hello" }]),
      ),
    );
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);

    const textarea = await screen.findByDisplayValue("Hello");
    expect(screen.queryByText(/^Speaker \d+$/)).not.toBeInTheDocument();
    expect(
      textarea.closest(".wp-speaker-block")?.querySelector(".wp-speaker-bar"),
    ).toHaveClass("wp-speaker-bar--none");
  });
});

describe("App — speaker rename", () => {
  function mockThreeSpeakerSegments() {
    vi.mocked(ipc.transcribeMeeting).mockResolvedValue(
      transcribeResult(
        transcribedMeeting([
          { start_ms: 0, end_ms: 1000, text: "Hi", speaker_id: 3 },
          { start_ms: 1000, end_ms: 2000, text: "There", speaker_id: 3 },
          { start_ms: 2000, end_ms: 3000, text: "Yo", speaker_id: 1 },
        ]),
      ),
    );
  }

  it("renames every segment sharing a speaker_id, leaving other speakers unchanged", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    mockThreeSpeakerSegments();
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hi");

    const [firstLabel] = screen.getAllByRole("button", {
      name: "Rename Speaker 4",
    });
    await user.click(firstLabel);
    const input = screen.getByRole("textbox", { name: "Rename Speaker 4" });
    await user.clear(input);
    await user.type(input, "Alice{Enter}");

    expect(screen.getAllByText("Alice")).toHaveLength(2);
    expect(screen.getByText("Speaker 2")).toBeInTheDocument();
  });

  it("includes the renamed label in the saved transcript, and leaves speaker-less segments bare", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.transcribeMeeting).mockResolvedValue(
      transcribeResult(
        transcribedMeeting([
          { start_ms: 0, end_ms: 1000, text: "Hi", speaker_id: 3 },
          { start_ms: 1000, end_ms: 2000, text: "No speaker here" },
        ]),
      ),
    );
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hi");

    await user.click(screen.getByRole("button", { name: "Rename Speaker 4" }));
    const renameInput = screen.getByRole("textbox", {
      name: "Rename Speaker 4",
    });
    await user.clear(renameInput);
    await user.type(renameInput, "Alice{Enter}");

    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(ipc.saveTextDialog).toHaveBeenCalledWith(
      "Alice: Hi\nNo speaker here",
      "meeting.txt",
    );
  });

  it("rejects an empty rename, keeping the previous label", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    mockThreeSpeakerSegments();
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hi");

    const [firstLabel] = screen.getAllByRole("button", {
      name: "Rename Speaker 4",
    });
    await user.click(firstLabel);
    const input = screen.getByRole("textbox", { name: "Rename Speaker 4" });
    await user.clear(input);
    await user.keyboard("{Enter}");

    expect(screen.getAllByText("Speaker 4")).toHaveLength(2);
  });

  it("cancels an in-progress edit on Escape without renaming", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    mockThreeSpeakerSegments();
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hi");

    const [firstLabel] = screen.getAllByRole("button", {
      name: "Rename Speaker 4",
    });
    await user.click(firstLabel);
    const input = screen.getByRole("textbox", { name: "Rename Speaker 4" });
    await user.type(input, "Bob");
    await user.keyboard("{Escape}");

    expect(screen.getAllByText("Speaker 4")).toHaveLength(2);
    expect(screen.queryByText("Bob")).not.toBeInTheDocument();
  });

  it("rejects a whitespace-only rename, keeping the previous label", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    mockThreeSpeakerSegments();
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hi");

    const [firstLabel] = screen.getAllByRole("button", {
      name: "Rename Speaker 4",
    });
    await user.click(firstLabel);
    const input = screen.getByRole("textbox", { name: "Rename Speaker 4" });
    await user.clear(input);
    await user.type(input, "   {Enter}");

    expect(screen.getAllByText("Speaker 4")).toHaveLength(2);
  });

  it("does not leak a rename into a later, unrelated transcript with a colliding speaker_id", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    mockThreeSpeakerSegments();
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hi");

    const [firstLabel] = screen.getAllByRole("button", {
      name: "Rename Speaker 4",
    });
    await user.click(firstLabel);
    const input = screen.getByRole("textbox", { name: "Rename Speaker 4" });
    await user.clear(input);
    await user.type(input, "Alice{Enter}");
    expect(screen.getAllByText("Alice")).toHaveLength(2);

    // Remove the file, then load a new, unrelated transcript whose first
    // segment happens to reuse speaker_id 3 - it must show the default
    // label, not the previous transcript's "Alice" rename.
    await user.click(screen.getByRole("button", { name: "Remove file" }));
    vi.mocked(ipc.transcribeMeeting).mockResolvedValue(
      transcribeResult(
        transcribedMeeting([
          { start_ms: 0, end_ms: 1000, text: "Fresh", speaker_id: 3 },
        ]),
      ),
    );
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Fresh");

    expect(screen.queryByText("Alice")).not.toBeInTheDocument();
    expect(screen.getByText("Speaker 4")).toBeInTheDocument();
  });
});
