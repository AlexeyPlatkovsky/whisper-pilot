import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import * as ipc from "./ipc";

const TRANSCRIPTION_DOWNLOADED = {
  id: "transcription",
  task: "transcription",
  label: "Whisper large-v3-turbo (Q8)",
  downloaded: true,
  size_bytes: 874_188_075,
};

const TRANSCRIPTION_NOT_DOWNLOADED = {
  ...TRANSCRIPTION_DOWNLOADED,
  downloaded: false,
};

vi.mock("./ipc", () => ({
  openFileDialog: vi.fn(async () => "/path/to/meeting.mp3"),
  transcribeFile: vi.fn(async () => ({
    file_name: "meeting.mp3",
    segments: [{ start_ms: 0, end_ms: 1000, text: "Hello" }],
  })),
  saveTextDialog: vi.fn(async () => null),
  listTaskModels: vi.fn(),
  downloadModel: vi.fn(),
  deleteModel: vi.fn(),
  onModelDownloadProgress: vi.fn(async () => () => {}),
  getSettings: vi.fn(async () => ({ theme: "system", ui_language: "en" })),
  setSetting: vi.fn(),
}));

async function waitForAddFileEnabled() {
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Add file" })).not.toBeDisabled(),
  );
}

// listTaskModels has no factory default (every test sets its own
// resolved/rejected value); reset it so a persistent implementation or a
// leftover "once" queue from one test can never leak into the next.
beforeEach(() => {
  vi.mocked(ipc.listTaskModels).mockReset();
  vi.mocked(ipc.openFileDialog).mockResolvedValue("/path/to/meeting.mp3");
  vi.mocked(ipc.transcribeFile).mockResolvedValue({
    file_name: "meeting.mp3",
    segments: [{ start_ms: 0, end_ms: 1000, text: "Hello" }],
  });
  vi.mocked(ipc.saveTextDialog).mockResolvedValue(null);
  vi.mocked(ipc.getSettings).mockResolvedValue({
    theme: "system",
    ui_language: "en",
  });
  delete document.documentElement.dataset.theme;
});

describe("App — Settings entry point", () => {
  it("opens Settings via the header gear with AI models selected by default", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole("button", { name: "Settings" }));

    expect(screen.getByRole("tab", { name: "AI models" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("closing Settings returns to the workspace with existing transcript state intact", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);
    await waitForAddFileEnabled();

    await user.click(screen.getByRole("button", { name: "Add file" }));
    await screen.findByDisplayValue("Hello");

    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: "Close settings" }));

    expect(screen.getByDisplayValue("Hello")).toBeInTheDocument();
  });

  it("keeps the gear in the header when other header content changes", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);
    await waitForAddFileEnabled();

    await user.click(screen.getByRole("button", { name: "Add file" }));
    await screen.findByDisplayValue("Hello");

    expect(
      screen.getByRole("button", { name: "Settings" }),
    ).toBeInTheDocument();
  });
});

describe("App — transcription model availability", () => {
  it("disables Add-file with an explanation when the transcription model is not downloaded", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([
      TRANSCRIPTION_NOT_DOWNLOADED,
    ]);
    render(<App />);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Add file" })).toBeDisabled(),
    );
    expect(screen.getByText(/whisper/i)).toBeInTheDocument();
  });

  it("enables Add-file when the transcription model is downloaded", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    render(<App />);

    await waitForAddFileEnabled();
  });

  it("fails safe (disabled, with the warning) when the availability check itself fails", async () => {
    vi.mocked(ipc.listTaskModels).mockRejectedValue(
      new Error("IPC unavailable"),
    );
    render(<App />);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Add file" })).toBeDisabled(),
    );
    expect(screen.getByText(/whisper/i)).toBeInTheDocument();
  });

  it("re-enables Add-file after closing Settings once the model has been downloaded", async () => {
    // Three calls happen, in order: App's own mount check, AiModelsSection's
    // mount check when Settings opens (it independently calls
    // listTaskModels too), then App's refresh when Settings closes.
    vi.mocked(ipc.listTaskModels)
      .mockResolvedValueOnce([TRANSCRIPTION_NOT_DOWNLOADED])
      .mockResolvedValueOnce([TRANSCRIPTION_NOT_DOWNLOADED])
      .mockResolvedValueOnce([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Add file" })).toBeDisabled(),
    );

    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: "Close settings" }));

    await waitForAddFileEnabled();
  });
});

describe("App — English strings", () => {
  it("renders the Save button in English", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    render(<App />);

    expect(
      await screen.findByRole("button", { name: "Save" }),
    ).toBeInTheDocument();
  });

  it("renders the empty-state message in English", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    render(<App />);

    expect(
      await screen.findByText(
        "Add an audio or video file to get a transcript.",
      ),
    ).toBeInTheDocument();
  });

  it("shows a compact transcribing status — label plus a ticking timer, no filename or blurb", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    let resolveTranscribe: (v: {
      file_name: string;
      segments: { start_ms: number; end_ms: number; text: string }[];
    }) => void = () => {};
    vi.mocked(ipc.transcribeFile).mockReturnValue(
      new Promise((resolve) => {
        resolveTranscribe = resolve;
      }),
    );
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = userEvent.setup({
        advanceTimers: vi.advanceTimersByTime.bind(vi),
      });

      render(<App />);
      await waitForAddFileEnabled();
      await user.click(screen.getByRole("button", { name: "Add file" }));

      // The transcribing state lives in the header status region (role="status").
      const status = await screen.findByRole("status");
      await waitFor(() => expect(status).toHaveTextContent("Transcribing"));

      // No filename, no "minutes" blurb — just the label and a mm:ss timer.
      expect(status.textContent).not.toContain("meeting.mp3");
      expect(status.textContent).not.toContain("minutes");
      expect(status.querySelector("b")).toBeNull();
      expect(status.querySelector(".wp-status-timer")?.textContent).toBe(
        "00:00",
      );

      // The timer ticks once per second.
      await vi.advanceTimersByTimeAsync(2000);
      expect(status.querySelector(".wp-status-timer")?.textContent).toBe(
        "00:02",
      );

      resolveTranscribe({
        file_name: "meeting.mp3",
        segments: [{ start_ms: 0, end_ms: 1000, text: "Hello" }],
      });
      await screen.findByDisplayValue("Hello");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("App — theme application", () => {
  it("applies the persisted theme on mount, before the user ever opens Settings", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "dark",
      ui_language: "en",
    });

    render(<App />);

    await waitFor(() =>
      expect(document.documentElement.dataset.theme).toBe("dark"),
    );
  });
});

describe("App — file handling", () => {
  it("does nothing when the file dialog is cancelled", async () => {
    vi.mocked(ipc.openFileDialog).mockResolvedValue(null);
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Add file" }));

    expect(screen.getByText("No file loaded")).toBeInTheDocument();
  });

  it("shows a model-missing warning when the transcription model is not downloaded", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([
      TRANSCRIPTION_NOT_DOWNLOADED,
    ]);
    render(<App />);

    expect(
      await screen.findByText(/The Whisper model isn't downloaded/i),
    ).toBeInTheDocument();
  });

  it("displays the transcription error when transcribeFile rejects", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.transcribeFile).mockRejectedValue(
      new Error("whisper failed"),
    );
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Add file" }));

    expect(await screen.findByText(/whisper failed/i)).toBeInTheDocument();
  });

  it("removes the attached file chip and clears the transcript", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Add file" }));
    await screen.findByDisplayValue("Hello");

    await user.click(screen.getByRole("button", { name: "Remove file" }));

    expect(screen.getByText("No file loaded")).toBeInTheDocument();
    expect(screen.queryByDisplayValue("Hello")).not.toBeInTheDocument();
  });

  it("saves the transcript when Save is clicked", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Add file" }));
    await screen.findByDisplayValue("Hello");

    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(ipc.saveTextDialog).toHaveBeenCalledWith("Hello", "meeting.txt");
  });
});

describe("App — sidebar", () => {
  it("toggles the sidebar when the toggle button is clicked", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitFor(() =>
      expect(screen.getByLabelText("Search meetings")).toBeInTheDocument(),
    );

    await user.click(screen.getByRole("button", { name: "Toggle sidebar" }));

    expect(screen.queryByLabelText("Search meetings")).not.toBeInTheDocument();
  });
});

describe("App — transcript editing", () => {
  it("updates a segment when the user types in its textarea", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Add file" }));
    const textarea = await screen.findByDisplayValue("Hello");

    await user.clear(textarea);
    await user.type(textarea, "Hi there");

    expect(screen.getByDisplayValue("Hi there")).toBeInTheDocument();
  });
});

describe("App — speaker rendering", () => {
  it("labels segments by their real speaker_id, not by array position", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    // Indices 0 and 1 share speaker_id 5 (must render the SAME label,
    // "Speaker 6"); index 2 is speaker_id 2 ("Speaker 3"). An index-based
    // fake (e.g. i % N) would instead print three different labels here,
    // so this fails against position-driven labeling, not just no labeling.
    vi.mocked(ipc.transcribeFile).mockResolvedValue({
      file_name: "meeting.mp3",
      segments: [
        { start_ms: 0, end_ms: 1000, text: "A", speaker_id: 5 },
        { start_ms: 1000, end_ms: 2000, text: "B", speaker_id: 5 },
        { start_ms: 2000, end_ms: 3000, text: "C", speaker_id: 2 },
      ],
    });
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Add file" }));

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
    vi.mocked(ipc.transcribeFile).mockResolvedValue({
      file_name: "meeting.mp3",
      segments: [{ start_ms: 0, end_ms: 1000, text: "Hello" }],
    });
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Add file" }));

    const textarea = await screen.findByDisplayValue("Hello");
    expect(screen.queryByText(/^Speaker \d+$/)).not.toBeInTheDocument();
    expect(
      textarea.closest(".wp-speaker-block")?.querySelector(".wp-speaker-bar"),
    ).toHaveClass("wp-speaker-bar--none");
  });
});
