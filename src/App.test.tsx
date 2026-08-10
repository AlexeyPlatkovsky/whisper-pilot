import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import * as ipc from "./ipc";
import type { Meeting, Segment, TranscribeMeetingResult } from "./ipc";
import { resolveMeetingStatus } from "./meetingStatus";

const TRANSCRIPTION_DOWNLOADED = {
  id: "transcription",
  task: "transcription",
  label: "Whisper large-v3-turbo (Q8)",
  downloaded: true,
  size_bytes: 874_188_075,
  recommended: false,
};

const TRANSCRIPTION_NOT_DOWNLOADED = {
  ...TRANSCRIPTION_DOWNLOADED,
  downloaded: false,
};

// The workspace is always backed by a real, persisted meeting. On an empty
// library the app seeds this one on mount; attaching a file and transcribing
// are separate, explicit steps that each return an updated meeting.
const EMPTY_MEETING: Meeting = {
  id: 100,
  title: "New Meeting",
  created_at_ms: 0,
  language: "ru",
  status: "no_files",
  segments: [],
  source_missing: false,
};

const ATTACHED_MEETING: Meeting = {
  ...EMPTY_MEETING,
  source_path: "/path/to/meeting.mp3",
  source_name: "meeting.mp3",
  status: "ready",
  segments: [],
};

function transcribedMeeting(segments: Segment[]): Meeting {
  return {
    ...ATTACHED_MEETING,
    status: "finished",
    duration_ms: segments.at(-1)?.end_ms ?? 0,
    segments,
  };
}

function transcribeResult(
  meeting: Meeting,
  diarizationWarning?: string,
): TranscribeMeetingResult {
  return { meeting, diarization_warning: diarizationWarning };
}

const HELLO_SEGMENT: Segment = { start_ms: 0, end_ms: 1000, text: "Hello" };

vi.mock("./ipc", () => ({
  openFileDialog: vi.fn(async () => "/path/to/meeting.mp3"),
  setMeetingSource: vi.fn(),
  transcribeMeeting: vi.fn(),
  diarizeMeeting: vi.fn(),
  createMeeting: vi.fn(),
  deleteMeeting: vi.fn(),
  listMeetings: vi.fn(),
  openMeeting: vi.fn(),
  renameMeeting: vi.fn(),
  updateSegment: vi.fn(),
  updateNotes: vi.fn(),
  cancelTranscription: vi.fn(),
  generateNotes: vi.fn(),
  saveTextDialog: vi.fn(async () => null),
  listTaskModels: vi.fn(),
  downloadModel: vi.fn(),
  deleteModel: vi.fn(),
  onModelDownloadProgress: vi.fn(async () => () => {}),
  onTranscriptionPhase: vi.fn(async () => () => {}),
  onTranscriptionProgress: vi.fn(async () => () => {}),
  getSettings: vi.fn(async () => ({
    theme: "system",
    ui_language: "en",
    active_model_diarization: "none",
    export_file_type: "plain_text",
  })),
  setSetting: vi.fn(),
  listStreamingSessions: vi.fn(async () => []),
  openStreamingSession: vi.fn(),
  renameStreamingSession: vi.fn(),
  deleteStreamingSession: vi.fn(),
  startStreamingSession: vi.fn(),
  stopStreamingSession: vi.fn(),
  onStreamingWindow: vi.fn(async () => () => {}),
  onStreamingSources: vi.fn(async () => () => {}),
  onStreamingSessionEnded: vi.fn(async () => () => {}),
}));

async function waitForAddFileEnabled() {
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Choose file" }),
    ).not.toBeDisabled(),
  );
}

// The default flow: attach a file, then explicitly transcribe it.
async function chooseAndTranscribe(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "Choose file" }));
  const transcribe = await screen.findByRole("button", { name: "Transcribe" });
  await waitFor(() => expect(transcribe).not.toBeDisabled());
  await user.click(transcribe);
}

// listTaskModels has no factory default (every test sets its own
// resolved/rejected value); reset it so a persistent implementation or a
// leftover "once" queue from one test can never leak into the next.
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(ipc.listTaskModels).mockReset();
  vi.mocked(ipc.openFileDialog).mockResolvedValue("/path/to/meeting.mp3");
  vi.mocked(ipc.createMeeting).mockResolvedValue({ ...EMPTY_MEETING });
  vi.mocked(ipc.setMeetingSource).mockImplementation(async (_id, path) =>
    path === null ? { ...EMPTY_MEETING } : { ...ATTACHED_MEETING },
  );
  vi.mocked(ipc.transcribeMeeting).mockResolvedValue(
    transcribeResult(transcribedMeeting([HELLO_SEGMENT])),
  );
  vi.mocked(ipc.saveTextDialog).mockResolvedValue(null);
  vi.mocked(ipc.listMeetings).mockResolvedValue([]);
  vi.mocked(ipc.deleteMeeting).mockReset();
  vi.mocked(ipc.openMeeting).mockReset();
  vi.mocked(ipc.renameMeeting).mockReset();
  vi.mocked(ipc.updateSegment).mockReset();
  vi.mocked(ipc.updateNotes).mockReset();
  vi.mocked(ipc.updateSegment).mockResolvedValue(
    transcribedMeeting([HELLO_SEGMENT]),
  );
  vi.mocked(ipc.updateNotes).mockImplementation(async (notes) => ({
    ...transcribedMeeting([HELLO_SEGMENT]),
    notes,
  }));
  vi.mocked(ipc.generateNotes).mockReset();
  vi.mocked(ipc.cancelTranscription).mockReset();
  vi.mocked(ipc.cancelTranscription).mockResolvedValue(undefined);
  vi.mocked(ipc.diarizeMeeting).mockReset();
  vi.mocked(ipc.getSettings).mockResolvedValue({
    theme: "system",
    ui_language: "en",
    active_model_diarization: "none",
    export_file_type: "plain_text",
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

    await chooseAndTranscribe(user);
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

    await chooseAndTranscribe(user);
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
      expect(
        screen.getByRole("button", { name: "Choose file" }),
      ).toBeDisabled(),
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
      expect(
        screen.getByRole("button", { name: "Choose file" }),
      ).toBeDisabled(),
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
      expect(
        screen.getByRole("button", { name: "Choose file" }),
      ).toBeDisabled(),
    );

    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: "Close settings" }));

    await waitForAddFileEnabled();
  });
});

describe("App — English strings", () => {
  it("shows the detected language only after a meeting has been transcribed", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    expect(screen.queryByText("Russian")).not.toBeInTheDocument();

    await chooseAndTranscribe(user);

    expect(await screen.findByText("Russian")).toBeInTheDocument();
  });

  it("shows the detected non-Russian language after transcription", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.transcribeMeeting).mockResolvedValue(
      transcribeResult({
        ...transcribedMeeting([HELLO_SEGMENT]),
        language: "en",
      }),
    );
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);

    expect(await screen.findByText("English")).toBeInTheDocument();
    expect(screen.queryByText("Russian")).not.toBeInTheDocument();
  });

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
    let resolveTranscribe: (result: TranscribeMeetingResult) => void = () => {};
    vi.mocked(ipc.transcribeMeeting).mockReturnValue(
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
      await user.click(screen.getByRole("button", { name: "Choose file" }));
      const transcribe = await screen.findByRole("button", {
        name: "Transcribe",
      });
      await waitFor(() => expect(transcribe).not.toBeDisabled());
      await user.click(transcribe);

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

      resolveTranscribe(transcribeResult(transcribedMeeting([HELLO_SEGMENT])));
      await screen.findByDisplayValue("Hello");
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows no progress bar before any transcription_progress event, then a determinate bar once one arrives", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.transcribeMeeting).mockReturnValue(new Promise(() => {}));
    let progressHandler: (p: {
      id: number;
      percent: number;
    }) => void = () => {};
    vi.mocked(ipc.onTranscriptionProgress).mockImplementation(
      async (handler) => {
        progressHandler = handler;
        return () => {};
      },
    );
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Choose file" }));
    const transcribe = await screen.findByRole("button", {
      name: "Transcribe",
    });
    await waitFor(() => expect(transcribe).not.toBeDisabled());
    await user.click(transcribe);

    const status = await screen.findByRole("status");
    await waitFor(() => expect(status).toHaveTextContent("Transcribing"));
    expect(
      status.querySelector(".wp-status-progress-bar"),
    ).not.toBeInTheDocument();

    progressHandler({ id: 100, percent: 42 });

    const bar = await waitFor(() => {
      const el = status.querySelector(".wp-status-progress-bar");
      expect(el).not.toBeNull();
      return el as HTMLProgressElement;
    });
    expect(bar.value).toBe(42);
    expect(status.querySelector(".wp-status-progress-label")).toHaveTextContent(
      "42%",
    );
  });

  it("clears the progress bar once the run moves into the diarizing phase", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.transcribeMeeting).mockReturnValue(new Promise(() => {}));
    let progressHandler: (p: {
      id: number;
      percent: number;
    }) => void = () => {};
    vi.mocked(ipc.onTranscriptionProgress).mockImplementation(
      async (handler) => {
        progressHandler = handler;
        return () => {};
      },
    );
    let phaseHandler: (p: {
      id: number;
      phase: "diarizing";
    }) => void = () => {};
    vi.mocked(ipc.onTranscriptionPhase).mockImplementation(async (handler) => {
      phaseHandler = handler;
      return () => {};
    });
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Choose file" }));
    const transcribe = await screen.findByRole("button", {
      name: "Transcribe",
    });
    await waitFor(() => expect(transcribe).not.toBeDisabled());
    await user.click(transcribe);

    const status = await screen.findByRole("status");
    progressHandler({ id: 100, percent: 80 });
    await waitFor(() =>
      expect(
        status.querySelector(".wp-status-progress-bar"),
      ).toBeInTheDocument(),
    );

    phaseHandler({ id: 100, phase: "diarizing" });

    await waitFor(() => expect(status).toHaveTextContent("Diarizing"));
    expect(
      status.querySelector(".wp-status-progress-bar"),
    ).not.toBeInTheDocument();
  });
});

describe("App — theme application", () => {
  it("applies the persisted theme on mount, before the user ever opens Settings", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "dark",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
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
    await user.click(screen.getByRole("button", { name: "Choose file" }));

    expect(ipc.setMeetingSource).not.toHaveBeenCalled();
    expect(screen.getByText("No file loaded")).toBeInTheDocument();
  });

  it("attaches a chosen file without transcribing until Transcribe is clicked", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Choose file" }));

    // File is attached, but no transcript is produced on its own.
    expect(await screen.findByText("meeting.mp3")).toBeInTheDocument();
    expect(ipc.transcribeMeeting).not.toHaveBeenCalled();
    expect(screen.queryByDisplayValue("Hello")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Transcribe" }));
    expect(await screen.findByDisplayValue("Hello")).toBeInTheDocument();
    expect(ipc.transcribeMeeting).toHaveBeenCalledWith(EMPTY_MEETING.id);
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

  it("displays the transcription error when transcribeMeeting rejects", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.transcribeMeeting).mockRejectedValue(
      new Error("whisper failed"),
    );
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);

    expect(await screen.findByText(/whisper failed/i)).toBeInTheDocument();
  });

  it("shows a blocking modal when transcription succeeds but diarization degraded, dismissed by OK", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.transcribeMeeting).mockResolvedValue(
      transcribeResult(
        transcribedMeeting([HELLO_SEGMENT]),
        "active diarization model is missing",
      ),
    );
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);

    const dialog = await screen.findByRole("alertdialog", {
      name: "Speaker identification issue",
    });
    expect(dialog).toHaveTextContent("active diarization model is missing");

    await user.click(screen.getByRole("button", { name: "OK" }));

    expect(
      screen.queryByRole("alertdialog", {
        name: "Speaker identification issue",
      }),
    ).not.toBeInTheDocument();
  });

  it("does not show the diarization warning modal when transcription succeeds cleanly", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hello");

    expect(
      screen.queryByRole("alertdialog", {
        name: "Speaker identification issue",
      }),
    ).not.toBeInTheDocument();
  });

  it("removes the attached file chip and clears the transcript", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hello");

    await user.click(screen.getByRole("button", { name: "Remove file" }));

    expect(ipc.setMeetingSource).toHaveBeenCalledWith(EMPTY_MEETING.id, null);
    expect(await screen.findByText("No file loaded")).toBeInTheDocument();
    expect(screen.queryByDisplayValue("Hello")).not.toBeInTheDocument();
  });

  it("saves the transcript when Save is clicked", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hello");

    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(ipc.saveTextDialog).toHaveBeenCalledWith("Hello", "meeting.txt");
  });

  it("saves as Markdown with a .md extension when that is the persisted export file type", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "markdown",
    });
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hello");

    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(ipc.saveTextDialog).toHaveBeenCalledWith(
      "# Transcript\n\n[0:00] Hello",
      "meeting.md",
    );
  });

  it("copies the Markdown rendering to the clipboard when that is the persisted export file type", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "markdown",
    });
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    render(<App />);

    await waitForAddFileEnabled();
    await chooseAndTranscribe(user);
    await screen.findByDisplayValue("Hello");

    await user.click(screen.getByRole("button", { name: "Copy transcript" }));

    expect(writeText).toHaveBeenCalledWith("# Transcript\n\n[0:00] Hello");
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

  it("filters meetings by title only after three characters and shows no matches", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: 2,
        title: "Roadmap review",
        created_at_ms: 2_000,
        status: "finished",
      },
      {
        id: 1,
        title: "Weekly sync",
        created_at_ms: 1_000,
        status: "finished",
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue({
      ...transcribedMeeting([HELLO_SEGMENT]),
      id: 2,
      title: "Roadmap review",
    });
    const user = userEvent.setup();
    render(<App />);

    const search = await screen.findByLabelText("Search meetings");
    const sidebar = search.closest("aside");
    expect(sidebar).not.toBeNull();
    // BVA: filtering starts at exactly three characters and resets at two.
    await user.type(search, "roa");
    expect(within(sidebar!).getByText("Roadmap review")).toBeInTheDocument();
    expect(within(sidebar!).queryByText("Weekly sync")).not.toBeInTheDocument();

    await user.type(search, "x");
    expect(await screen.findByText("No matches")).toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "ro");
    expect(within(sidebar!).getByText("Roadmap review")).toBeInTheDocument();
    expect(within(sidebar!).getByText("Weekly sync")).toBeInTheDocument();
  });
});

describe("App — persisted meeting workspace", () => {
  const NEWEST_MEETING = {
    id: 2,
    title: "Newest meeting",
    created_at_ms: 2_000,
    duration_ms: 1_000,
    language: "ru",
    status: "finished",
    segments: [{ start_ms: 0, end_ms: 1_000, text: "Saved transcript" }],
    source_missing: false,
  };

  it("opens the newest persisted meeting on startup without rendering sample rows", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: NEWEST_MEETING.id,
        title: NEWEST_MEETING.title,
        created_at_ms: NEWEST_MEETING.created_at_ms,
        duration_ms: NEWEST_MEETING.duration_ms,
        status: NEWEST_MEETING.status,
      },
      {
        id: 1,
        title: "Older meeting",
        created_at_ms: 1_000,
        status: "finished",
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue(NEWEST_MEETING);

    render(<App />);

    expect(
      await screen.findByRole("heading", { name: "Newest meeting" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByDisplayValue("Saved transcript"),
    ).toBeInTheDocument();
    expect(screen.getByText("Older meeting")).toBeInTheDocument();
    expect(screen.queryByText("Product Standup")).not.toBeInTheDocument();
    // A non-empty library must not seed an extra meeting.
    expect(ipc.createMeeting).not.toHaveBeenCalled();
  });

  // state-transition: idle → copied → idle (timeout rollback).
  it("shows a top 'Copied' toast and a checked button after copying, then rolls back", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: NEWEST_MEETING.id,
        title: NEWEST_MEETING.title,
        created_at_ms: NEWEST_MEETING.created_at_ms,
        status: NEWEST_MEETING.status,
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue(NEWEST_MEETING);
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
      render(<App />);
      await screen.findByDisplayValue("Saved transcript");

      await user.click(screen.getByRole("button", { name: "Copy transcript" }));

      // Flush the click's async continuation deterministically rather than
      // polling with `findByText`: `shouldAdvanceTime` folds real wall-clock
      // into the fake clock, so on a slow CI runner the 2500ms rollback timer
      // could fire before the poll ever observes the toast.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      const toast = screen.getByText("Copied", { selector: ".wp-toast" });
      expect(toast).toHaveAttribute("role", "status");
      expect(toast).toHaveClass("wp-toast--top");
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

  // state-transition: copied --switch meeting--> idle (pending feedback cleared).
  it("clears a pending Copied feedback when another meeting is opened", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const OLDER_MEETING = {
      id: 1,
      title: "Older meeting",
      created_at_ms: 1_000,
      duration_ms: 500,
      language: "ru",
      status: "finished",
      segments: [{ start_ms: 0, end_ms: 500, text: "Older transcript" }],
      source_missing: false,
    };
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: NEWEST_MEETING.id,
        title: NEWEST_MEETING.title,
        created_at_ms: NEWEST_MEETING.created_at_ms,
        status: NEWEST_MEETING.status,
      },
      {
        id: OLDER_MEETING.id,
        title: OLDER_MEETING.title,
        created_at_ms: OLDER_MEETING.created_at_ms,
        status: OLDER_MEETING.status,
      },
    ]);
    vi.mocked(ipc.openMeeting).mockImplementation(async (id) =>
      id === OLDER_MEETING.id ? OLDER_MEETING : NEWEST_MEETING,
    );
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
      render(<App />);
      await screen.findByDisplayValue("Saved transcript");

      await user.click(screen.getByRole("button", { name: "Copy transcript" }));
      // Same deterministic flush as the first toast test: no polling across
      // real time that `shouldAdvanceTime` folds into the fake clock.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      screen.getByText("Copied", { selector: ".wp-toast" });

      await user.click(
        screen.getByRole("button", { name: "Open Older meeting" }),
      );
      await screen.findByDisplayValue("Older transcript");

      expect(
        screen.queryByText("Copied", { selector: ".wp-toast" }),
      ).not.toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Copy transcript" }),
      ).toBeInTheDocument();

      // The cancelled timer must not resurrect the feedback on the new meeting.
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

  // state-transition: copying (in-flight write) --switch meeting--> the late
  // resolution must not enter the copied state on the new meeting.
  it("does not paint Copied feedback onto a meeting opened while the clipboard write is in flight", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const OLDER_MEETING = {
      id: 1,
      title: "Older meeting",
      created_at_ms: 1_000,
      duration_ms: 500,
      language: "ru",
      status: "finished",
      segments: [{ start_ms: 0, end_ms: 500, text: "Older transcript" }],
      source_missing: false,
    };
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: NEWEST_MEETING.id,
        title: NEWEST_MEETING.title,
        created_at_ms: NEWEST_MEETING.created_at_ms,
        status: NEWEST_MEETING.status,
      },
      {
        id: OLDER_MEETING.id,
        title: OLDER_MEETING.title,
        created_at_ms: OLDER_MEETING.created_at_ms,
        status: OLDER_MEETING.status,
      },
    ]);
    vi.mocked(ipc.openMeeting).mockImplementation(async (id) =>
      id === OLDER_MEETING.id ? OLDER_MEETING : NEWEST_MEETING,
    );
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
    render(<App />);
    await screen.findByDisplayValue("Saved transcript");

    await user.click(screen.getByRole("button", { name: "Copy transcript" }));
    // The write is still in flight: no feedback yet.
    expect(
      screen.queryByText("Copied", { selector: ".wp-toast" }),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Open Older meeting" }),
    );
    await screen.findByDisplayValue("Older transcript");

    // The write for the previous meeting resolves only now.
    resolveWrite();
    await act(async () => {});

    expect(
      screen.queryByText("Copied", { selector: ".wp-toast" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Copy transcript" }),
    ).toBeInTheDocument();
  });

  it("a failed clipboard write surfaces an error and shows no Copied feedback", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: NEWEST_MEETING.id,
        title: NEWEST_MEETING.title,
        created_at_ms: NEWEST_MEETING.created_at_ms,
        status: NEWEST_MEETING.status,
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue(NEWEST_MEETING);
    const user = userEvent.setup();
    // Installed after userEvent.setup, which swaps navigator.clipboard for
    // its own stub — defining ours last is what the component actually calls.
    const writeText = vi.fn().mockRejectedValue("denied");
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    render(<App />);
    await screen.findByDisplayValue("Saved transcript");

    await user.click(screen.getByRole("button", { name: "Copy transcript" }));

    const status = await screen.findByRole("status");
    await waitFor(() => expect(status).toHaveTextContent("Error"));
    expect(
      screen.queryByText("Copied", { selector: ".wp-toast" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Copy transcript" }),
    ).toBeInTheDocument();
  });

  it("seeds a single New Meeting when the library is empty, without fake sample rows", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    // listMeetings defaults to [] in beforeEach.

    render(<App />);

    expect(
      await screen.findByRole("heading", { name: "New Meeting" }),
    ).toBeInTheDocument();
    expect(ipc.createMeeting).toHaveBeenCalledOnce();
    expect(screen.queryByText("No meetings yet")).not.toBeInTheDocument();
    expect(screen.queryByText("Product Standup")).not.toBeInTheDocument();
  });

  it("creates and selects an empty meeting without opening a file dialog or starting transcription", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    // Start from a non-empty library so no meeting is auto-seeded on mount.
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: 1,
        title: "Older meeting",
        created_at_ms: 1_000,
        status: "finished",
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue({
      id: 1,
      title: "Older meeting",
      created_at_ms: 1_000,
      language: "ru",
      status: "finished",
      segments: [],
      source_missing: false,
    });
    vi.mocked(ipc.createMeeting).mockResolvedValue({
      id: 3,
      title: "New Meeting",
      created_at_ms: 3_000,
      language: "ru",
      status: "no_files",
      segments: [],
      source_missing: false,
    });
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "Older meeting" });
    await user.click(screen.getByRole("button", { name: "New meeting" }));

    expect(ipc.createMeeting).toHaveBeenCalledOnce();
    expect(ipc.openFileDialog).not.toHaveBeenCalled();
    expect(ipc.transcribeMeeting).not.toHaveBeenCalled();
    expect(
      screen.getByRole("heading", { name: "New Meeting" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("No files");
  });

  it("[state-transition] retains the active meeting when opening another meeting fails", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: NEWEST_MEETING.id,
        title: NEWEST_MEETING.title,
        created_at_ms: NEWEST_MEETING.created_at_ms,
        status: NEWEST_MEETING.status,
      },
      {
        id: 1,
        title: "Unavailable meeting",
        created_at_ms: 1_000,
        status: "finished",
      },
    ]);
    vi.mocked(ipc.openMeeting)
      .mockResolvedValueOnce(NEWEST_MEETING)
      .mockRejectedValueOnce(new Error("meeting is unavailable"));
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "Newest meeting" });
    await user.click(
      screen.getByRole("button", { name: "Open Unavailable meeting" }),
    );

    expect(
      screen.getByRole("heading", { name: "Newest meeting" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "meeting is unavailable",
    );
  });

  it("[state-transition] retains the active meeting when creating one fails", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: NEWEST_MEETING.id,
        title: NEWEST_MEETING.title,
        created_at_ms: NEWEST_MEETING.created_at_ms,
        status: NEWEST_MEETING.status,
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue(NEWEST_MEETING);
    vi.mocked(ipc.createMeeting).mockRejectedValue(new Error("disk full"));
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "Newest meeting" });
    await user.click(screen.getByRole("button", { name: "New meeting" }));

    expect(
      screen.getByRole("heading", { name: "Newest meeting" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("disk full");
  });

  it("switches focus to a newly created meeting instead of staying on the previous one", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: NEWEST_MEETING.id,
        title: NEWEST_MEETING.title,
        created_at_ms: NEWEST_MEETING.created_at_ms,
        status: NEWEST_MEETING.status,
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue(NEWEST_MEETING);
    vi.mocked(ipc.createMeeting).mockResolvedValue({
      id: 9,
      title: "New Meeting",
      created_at_ms: 9_000,
      language: "ru",
      status: "no_files",
      segments: [],
      source_missing: false,
    });
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "Newest meeting" });
    await user.click(screen.getByRole("button", { name: "New meeting" }));

    // The workspace now shows the new meeting, and the previous one is still
    // listed in the sidebar (nothing is lost).
    expect(
      await screen.findByRole("heading", { name: "New Meeting" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Open Newest meeting" }),
    ).toBeInTheDocument();
  });
});

describe("App — source file missing", () => {
  const MISSING_SOURCE_MEETING: Meeting = {
    ...transcribedMeeting([HELLO_SEGMENT]),
    source_missing: true,
  };

  it("disables Transcribe and shows an explanatory note, but keeps the transcript editable", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: MISSING_SOURCE_MEETING.id,
        title: MISSING_SOURCE_MEETING.title,
        created_at_ms: MISSING_SOURCE_MEETING.created_at_ms,
        duration_ms: MISSING_SOURCE_MEETING.duration_ms,
        status: MISSING_SOURCE_MEETING.status,
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue(MISSING_SOURCE_MEETING);

    render(<App />);

    await screen.findByText(/Source file missing/);
    expect(screen.getByRole("button", { name: "Transcribe" })).toBeDisabled();

    const textarea = screen.getByDisplayValue("Hello");
    expect(textarea).not.toBeDisabled();
  });

  it("does not show the note and keeps Transcribe available when the source file exists", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: ATTACHED_MEETING.id,
        title: ATTACHED_MEETING.title,
        created_at_ms: ATTACHED_MEETING.created_at_ms,
        duration_ms: ATTACHED_MEETING.duration_ms,
        status: ATTACHED_MEETING.status,
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue(ATTACHED_MEETING);

    render(<App />);

    await screen.findByRole("heading", { name: ATTACHED_MEETING.title });
    expect(screen.queryByText(/Source file missing/)).not.toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Transcribe" }),
      ).not.toBeDisabled(),
    );
  });
});

describe("App — persisted meeting controls", () => {
  const ACTIVE_MEETING = {
    id: 2,
    title: "Quarterly planning",
    created_at_ms: 2_000,
    language: "ru",
    status: "finished",
    segments: [{ start_ms: 0, end_ms: 1_000, text: "Saved transcript" }],
    source_missing: false,
  };

  function arrangeActiveMeeting() {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: ACTIVE_MEETING.id,
        title: ACTIVE_MEETING.title,
        created_at_ms: ACTIVE_MEETING.created_at_ms,
        status: ACTIVE_MEETING.status,
      },
      {
        id: 1,
        title: "Older meeting",
        created_at_ms: 1_000,
        status: "finished",
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue(ACTIVE_MEETING);
  }

  it("renames the active meeting from the header and updates the sidebar", async () => {
    arrangeActiveMeeting();
    vi.mocked(ipc.renameMeeting).mockResolvedValue({
      ...ACTIVE_MEETING,
      title: "Roadmap review",
    });
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: ACTIVE_MEETING.title });
    await user.click(screen.getByRole("button", { name: "Rename meeting" }));
    const dialog = screen.getByRole("dialog", { name: "Rename meeting" });
    const input = within(dialog).getByRole("textbox", {
      name: "Meeting label",
    });
    await user.clear(input);
    await user.type(input, "Roadmap review");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(ipc.renameMeeting).toHaveBeenCalledWith(2, "Roadmap review");
    expect(
      await screen.findByRole("heading", { name: "Roadmap review" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Open Roadmap review" }),
    ).toBeInTheDocument();
  });

  it("[EP + BVA] rejects blank and 121-character titles without writing", async () => {
    arrangeActiveMeeting();
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: ACTIVE_MEETING.title });
    await user.click(screen.getByRole("button", { name: "Rename meeting" }));
    const dialog = screen.getByRole("dialog", { name: "Rename meeting" });
    const input = within(dialog).getByRole("textbox", {
      name: "Meeting label",
    });
    await user.clear(input);
    await user.type(input, "   ");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "Meeting label is required",
    );

    await user.clear(input);
    await user.type(input, "a".repeat(121));
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "Meeting label must be 120 characters or fewer",
    );
    expect(ipc.renameMeeting).not.toHaveBeenCalled();
  });

  it("opens the same rename and delete dialogs from a sidebar row", async () => {
    arrangeActiveMeeting();
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: ACTIVE_MEETING.title });
    await user.click(
      screen.getByRole("button", { name: "Rename Older meeting" }),
    );
    expect(
      within(screen.getByRole("dialog", { name: "Rename meeting" })).getByRole(
        "textbox",
        { name: "Meeting label" },
      ),
    ).toHaveValue("Older meeting");
    await user.keyboard("{Escape}");

    await user.click(
      screen.getByRole("button", { name: "Delete Older meeting" }),
    );
    expect(
      screen.getByRole("alertdialog", { name: "Delete Older meeting" }),
    ).toBeInTheDocument();
  });

  it("requires confirmation before deleting the active meeting and opens the next newest one", async () => {
    arrangeActiveMeeting();
    const olderMeeting = {
      ...ACTIVE_MEETING,
      id: 1,
      title: "Older meeting",
      created_at_ms: 1_000,
    };
    vi.mocked(ipc.openMeeting)
      .mockResolvedValueOnce(ACTIVE_MEETING)
      .mockResolvedValueOnce(olderMeeting);
    vi.mocked(ipc.deleteMeeting).mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: ACTIVE_MEETING.title });
    await user.click(screen.getByRole("button", { name: "Delete meeting" }));
    const dialog = screen.getByRole("alertdialog", {
      name: `Delete ${ACTIVE_MEETING.title}`,
    });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(ipc.deleteMeeting).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Delete meeting" }));
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Delete",
      }),
    );

    expect(ipc.deleteMeeting).toHaveBeenCalledWith(2);
    expect(
      await screen.findByRole("heading", { name: "Older meeting" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Quarterly planning/ }),
    ).not.toBeInTheDocument();
  });

  it("seeds a fresh meeting after the last one is deleted", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: ACTIVE_MEETING.id,
        title: ACTIVE_MEETING.title,
        created_at_ms: ACTIVE_MEETING.created_at_ms,
        status: ACTIVE_MEETING.status,
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue(ACTIVE_MEETING);
    vi.mocked(ipc.deleteMeeting).mockResolvedValue(undefined);
    vi.mocked(ipc.createMeeting).mockResolvedValue({
      id: 5,
      title: "New Meeting",
      created_at_ms: 5_000,
      language: "ru",
      status: "no_files",
      segments: [],
      source_missing: false,
    });
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: ACTIVE_MEETING.title });
    await user.click(screen.getByRole("button", { name: "Delete meeting" }));
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Delete",
      }),
    );

    expect(ipc.deleteMeeting).toHaveBeenCalledWith(2);
    expect(ipc.createMeeting).toHaveBeenCalledOnce();
    expect(
      await screen.findByRole("heading", { name: "New Meeting" }),
    ).toBeInTheDocument();
  });
});

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
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const user = userEvent.setup({
        advanceTimers: vi.advanceTimersByTime.bind(vi),
      });
      render(<App />);
      await waitForAddFileEnabled();
      await chooseAndTranscribe(user);
      const textarea = await screen.findByDisplayValue("Hello");

      await user.type(textarea, "!");
      await vi.advanceTimersByTimeAsync(200);
      await user.type(textarea, "!");
      await vi.advanceTimersByTimeAsync(500);

      expect(ipc.updateSegment).toHaveBeenCalledTimes(1);
      expect(ipc.updateSegment).toHaveBeenCalledWith(100, 0, "Hello!!");
    } finally {
      vi.useRealTimers();
    }
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

describe("App — cancel transcription (Stop)", () => {
  it("keeps Stop disabled until a transcription is running, then wires it to cancelTranscription", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    let resolveTranscribe: (result: TranscribeMeetingResult) => void = () => {};
    vi.mocked(ipc.transcribeMeeting).mockReturnValue(
      new Promise((resolve) => {
        resolveTranscribe = resolve;
      }),
    );
    const user = userEvent.setup();
    render(<App />);

    const stop = screen.getByRole("button", { name: "Stop" });
    expect(stop).toBeDisabled();

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Choose file" }));
    const transcribe = await screen.findByRole("button", {
      name: "Transcribe",
    });
    await waitFor(() => expect(transcribe).not.toBeDisabled());
    await user.click(transcribe);

    await waitFor(() => expect(stop).not.toBeDisabled());
    await user.click(stop);
    expect(ipc.cancelTranscription).toHaveBeenCalledWith(100);
    void resolveTranscribe;
  });

  it("disables Stop once the run reaches the diarizing phase, which has no cancel hook", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    let phaseHandler: (p: {
      id: number;
      phase: "diarizing";
    }) => void = () => {};
    vi.mocked(ipc.onTranscriptionPhase).mockImplementation(async (handler) => {
      phaseHandler = handler;
      return () => {};
    });
    vi.mocked(ipc.transcribeMeeting).mockReturnValue(new Promise(() => {}));
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Choose file" }));
    const transcribe = await screen.findByRole("button", {
      name: "Transcribe",
    });
    await waitFor(() => expect(transcribe).not.toBeDisabled());
    await user.click(transcribe);

    const stop = screen.getByRole("button", { name: "Stop" });
    await waitFor(() => expect(stop).not.toBeDisabled());

    phaseHandler({ id: 100, phase: "diarizing" });

    await waitFor(() => expect(stop).toBeDisabled());
  });

  it("surfaces a cancelled run as a message, creates no document, and re-enables Transcribe", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.transcribeMeeting).mockRejectedValue(
      new Error("transcription stopped"),
    );
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Choose file" }));
    const transcribe = await screen.findByRole("button", {
      name: "Transcribe",
    });
    await waitFor(() => expect(transcribe).not.toBeDisabled());
    await user.click(transcribe);

    await screen.findByText(/transcription stopped/);
    expect(screen.queryByDisplayValue("Hello")).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled(),
    );
    expect(
      screen.getByRole("button", { name: "Transcribe" }),
    ).not.toBeDisabled();
  });

  it("surfaces a cancel-request failure without disturbing the in-flight run", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    let resolveTranscribe: (result: TranscribeMeetingResult) => void = () => {};
    vi.mocked(ipc.transcribeMeeting).mockReturnValue(
      new Promise((resolve) => {
        resolveTranscribe = resolve;
      }),
    );
    vi.mocked(ipc.cancelTranscription).mockRejectedValue(
      new Error("no transcription is running for meeting 100"),
    );
    const user = userEvent.setup();
    render(<App />);

    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Choose file" }));
    const transcribe = await screen.findByRole("button", {
      name: "Transcribe",
    });
    await waitFor(() => expect(transcribe).not.toBeDisabled());
    await user.click(transcribe);

    const stop = screen.getByRole("button", { name: "Stop" });
    await waitFor(() => expect(stop).not.toBeDisabled());
    await user.click(stop);

    await screen.findByText(/no transcription is running/);
    // The run itself is still in flight — resolving it completes normally.
    resolveTranscribe(transcribeResult(transcribedMeeting([HELLO_SEGMENT])));
    await screen.findByDisplayValue("Hello");
  });
});

const DIARIZATION_DOWNLOADED = {
  id: "diarization-campplus",
  task: "diarization",
  label: "CAM++",
  downloaded: true,
  size_bytes: 1,
  recommended: false,
};

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

describe("App — meeting status consistency", () => {
  // One library covering every persisted status the store can produce, so a
  // single render can prove the header, the row dot and the row label all read
  // from the same resolver.
  const READY_ACTIVE: Meeting = {
    id: 1,
    title: "Ready meeting",
    created_at_ms: 4_000,
    language: "ru",
    status: "ready",
    source_path: "/path/to/meeting.mp3",
    source_name: "meeting.mp3",
    segments: [],
    source_missing: false,
  };

  const READY_OTHER: Meeting = {
    ...READY_ACTIVE,
    id: 2,
    title: "Other ready meeting",
    created_at_ms: 3_000,
  };

  const FINISHED: Meeting = {
    id: 3,
    title: "Archived meeting",
    created_at_ms: 2_000,
    duration_ms: 1_000,
    language: "ru",
    status: "finished",
    segments: [HELLO_SEGMENT],
    source_missing: false,
  };

  const NO_FILES: Meeting = {
    id: 4,
    title: "Empty meeting",
    created_at_ms: 1_000,
    language: "ru",
    status: "no_files",
    segments: [],
    source_missing: false,
  };

  /** What READY_ACTIVE becomes once its transcription succeeds. */
  const ACTIVE_FINISHED: Meeting = {
    ...READY_ACTIVE,
    status: "finished",
    duration_ms: 1_000,
    segments: [HELLO_SEGMENT],
  };

  const LIBRARY = [READY_ACTIVE, READY_OTHER, FINISHED, NO_FILES];

  function arrangeLibrary() {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue(
      LIBRARY.map((meeting) => ({
        id: meeting.id,
        title: meeting.title,
        created_at_ms: meeting.created_at_ms,
        duration_ms: meeting.duration_ms,
        status: meeting.status,
      })),
    );
    vi.mocked(ipc.openMeeting).mockImplementation(async (id) => {
      const found = LIBRARY.find((meeting) => meeting.id === id);
      if (!found) throw new Error(`no meeting ${id}`);
      return { ...found };
    });
  }

  function row(title: string) {
    return screen.getByRole("listitem", { name: title });
  }

  function dot(title: string) {
    return row(title).querySelector(".wp-meeting-dot");
  }

  /** Hold a transcription open so the in-flight UI can be inspected. */
  function deferTranscription() {
    let resolveTranscribe: (result: TranscribeMeetingResult) => void = () => {};
    vi.mocked(ipc.transcribeMeeting).mockReturnValue(
      new Promise<TranscribeMeetingResult>((resolve) => {
        resolveTranscribe = resolve;
      }),
    );
    return (meeting: Meeting = { ...ACTIVE_FINISHED }) =>
      resolveTranscribe(transcribeResult(meeting));
  }

  async function startTranscribing(user: ReturnType<typeof userEvent.setup>) {
    await screen.findByRole("heading", { name: READY_ACTIVE.title });
    const transcribe = screen.getByRole("button", { name: "Transcribe" });
    await waitFor(() => expect(transcribe).not.toBeDisabled());
    await user.click(transcribe);
  }

  it("gives the header its label and each row dot its tone, from one resolver", async () => {
    arrangeLibrary();
    render(<App />);

    await screen.findByRole("heading", { name: READY_ACTIVE.title });

    // The header describes the active meeting and must agree with its row.
    expect(screen.getByRole("status")).toHaveTextContent("Ready");
    expect(dot(READY_ACTIVE.title)).toHaveClass("wp-tone--ready");
    expect(dot(FINISHED.title)).toHaveClass("wp-tone--finished");
    expect(dot(NO_FILES.title)).toHaveClass("wp-tone--no-files");

    // No row may fall back to the raw store value.
    expect(screen.queryByText("no_files")).not.toBeInTheDocument();
    expect(screen.queryByText("finished")).not.toBeInTheDocument();
  });

  it("shows an icon alongside the Meeting Ready status", async () => {
    arrangeLibrary();
    render(<App />);

    await screen.findByRole("heading", { name: READY_ACTIVE.title });

    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Ready");
    expect(status.querySelector("svg")).not.toBeNull();
  });

  it("carries the row status on the dot alone — no status text in the row", async () => {
    arrangeLibrary();
    render(<App />);

    await screen.findByRole("heading", { name: READY_ACTIVE.title });

    // The row is down to title/date/duration; the status word is gone from it.
    for (const meeting of [READY_ACTIVE, FINISHED, NO_FILES]) {
      const { label } = resolveMeetingStatus(meeting.status);
      expect(within(row(meeting.title)).queryByText(label)).toBeNull();
    }

    // The dot is what states the status now — as a hover hint and to a
    // screen reader, so dropping the text costs neither audience the meaning.
    expect(dot(READY_ACTIVE.title)).toHaveAttribute("title", "Ready");
    expect(dot(FINISHED.title)).toHaveAttribute("title", "Finished");
    expect(dot(NO_FILES.title)).toHaveAttribute("title", "No files");
    expect(
      within(row(NO_FILES.title)).getByRole("img", { name: "No files" }),
    ).toBe(dot(NO_FILES.title));
  });

  it("keeps a truncated meeting title readable on hover", async () => {
    // The reported regression: this name used to widen the whole sidebar.
    // The row now clips it, so the full title has to stay reachable somewhere.
    // Only the tooltip is assertable here — jsdom lays nothing out, so the
    // ellipsis and the pinned rail width are covered by the manual AX
    // measurement in the route's Manual UI verification record instead.
    const LONG_TITLE = "Test Meeting with a wide label more than we can show";
    const longMeeting: Meeting = { ...READY_ACTIVE, title: LONG_TITLE };
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: longMeeting.id,
        title: longMeeting.title,
        created_at_ms: longMeeting.created_at_ms,
        status: longMeeting.status,
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue({ ...longMeeting });
    render(<App />);

    await screen.findByRole("heading", { name: LONG_TITLE });
    expect(within(row(LONG_TITLE)).getByText(LONG_TITLE)).toHaveAttribute(
      "title",
      LONG_TITLE,
    );
  });

  it("keeps the header tone in step with the meeting the user opened", async () => {
    arrangeLibrary();
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: READY_ACTIVE.title });
    await user.click(
      screen.getByRole("button", { name: `Open ${NO_FILES.title}` }),
    );

    await screen.findByRole("heading", { name: NO_FILES.title });
    const header = screen.getByRole("status");
    expect(header).toHaveTextContent("No files");
    expect(within(header).getByText("No files")).toHaveClass(
      "wp-tone--no-files",
    );
  });

  it("shows a spinner in place of the row actions while that meeting transcribes", async () => {
    arrangeLibrary();
    const finish = deferTranscription();
    const user = userEvent.setup();
    render(<App />);

    await startTranscribing(user);

    const active = row(READY_ACTIVE.title);
    expect(
      await within(active).findByRole("img", { name: "Transcribing" }),
    ).toBeInTheDocument();
    expect(dot(READY_ACTIVE.title)).toHaveClass("wp-tone--transcribing");
    expect(dot(READY_ACTIVE.title)).toHaveAttribute("title", "Transcribing");
    // One accessible status per row: the dot states it, the spinner is purely
    // the visual half of the same fact.
    expect(
      within(active).getAllByRole("img", { name: "Transcribing" }),
    ).toHaveLength(1);
    expect(active.querySelector(".wp-meeting-busy .wp-spin")).not.toBeNull();
    // The spinner replaces the row's rename/delete group, not the status text.
    expect(
      within(active).queryByRole("button", {
        name: `Rename ${READY_ACTIVE.title}`,
      }),
    ).not.toBeInTheDocument();
    expect(
      within(active).queryByRole("button", {
        name: `Delete ${READY_ACTIVE.title}`,
      }),
    ).not.toBeInTheDocument();

    // Every other row is untouched: no spinner, actions intact.
    const idle = row(READY_OTHER.title);
    expect(
      within(idle).queryByRole("img", { name: "Transcribing" }),
    ).not.toBeInTheDocument();
    expect(
      within(idle).getByRole("button", { name: `Rename ${READY_OTHER.title}` }),
    ).toBeInTheDocument();

    finish();
    await waitFor(() =>
      expect(
        within(row(READY_ACTIVE.title)).queryByRole("img", {
          name: "Transcribing",
        }),
      ).not.toBeInTheDocument(),
    );
  });

  it("[state-transition] keeps the transcribing status after switching away and back", async () => {
    arrangeLibrary();
    const finish = deferTranscription();
    const user = userEvent.setup();
    render(<App />);

    await startTranscribing(user);
    await within(row(READY_ACTIVE.title)).findByRole("img", {
      name: "Transcribing",
    });

    // Away: the header describes the meeting the user is now looking at.
    await user.click(
      screen.getByRole("button", { name: `Open ${NO_FILES.title}` }),
    );
    await screen.findByRole("heading", { name: NO_FILES.title });
    expect(screen.getByRole("status")).toHaveTextContent("No files");
    // The run itself is untouched by navigation.
    expect(
      within(row(READY_ACTIVE.title)).getByRole("img", {
        name: "Transcribing",
      }),
    ).toBeInTheDocument();

    // Back: the still-running transcription is reported again, with its timer.
    await user.click(
      screen.getByRole("button", { name: `Open ${READY_ACTIVE.title}` }),
    );
    await screen.findByRole("heading", { name: READY_ACTIVE.title });
    const header = screen.getByRole("status");
    await waitFor(() => expect(header).toHaveTextContent("Transcribing"));
    // The elapsed clock is back and still counting.
    expect(header).toHaveTextContent(/\d{2}:\d{2}/);
    // The header spinner is decorative — the adjacent "Transcribing" text
    // already carries the meaning — so it has no accessible name to query by.
    expect(header.querySelector(".wp-spin")).not.toBeNull();

    finish();
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("Finished"),
    );
  });

  it("keeps Transcribe disabled on every meeting while a transcription is in flight", async () => {
    arrangeLibrary();
    const finish = deferTranscription();
    const user = userEvent.setup();
    render(<App />);

    await startTranscribing(user);
    expect(screen.getByRole("button", { name: "Transcribe" })).toBeDisabled();

    // READY_OTHER also has a source file, so only the in-flight run may keep
    // its Transcribe disabled.
    await user.click(
      screen.getByRole("button", { name: `Open ${READY_OTHER.title}` }),
    );
    await screen.findByRole("heading", { name: READY_OTHER.title });
    expect(screen.getByRole("button", { name: "Transcribe" })).toBeDisabled();

    finish();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Transcribe" }),
      ).not.toBeDisabled(),
    );
  });

  it("switches the header and row to Diarizing once the phase event fires, then back on completion", async () => {
    arrangeLibrary();
    const finish = deferTranscription();
    let phaseHandler: (p: {
      id: number;
      phase: "diarizing";
    }) => void = () => {};
    vi.mocked(ipc.onTranscriptionPhase).mockImplementation(async (handler) => {
      phaseHandler = handler;
      return () => {};
    });
    const user = userEvent.setup();
    render(<App />);

    await startTranscribing(user);
    expect(screen.getByRole("status")).toHaveTextContent("Transcribing");
    expect(
      within(row(READY_ACTIVE.title)).getByRole("img", {
        name: "Transcribing",
      }),
    ).toBeInTheDocument();

    phaseHandler({ id: READY_ACTIVE.id, phase: "diarizing" });

    const header = screen.getByRole("status");
    await waitFor(() => expect(header).toHaveTextContent("Diarizing"));
    expect(within(header).getByText("Diarizing")).toHaveClass(
      "wp-tone--diarizing",
    );
    expect(
      within(row(READY_ACTIVE.title)).getByRole("img", {
        name: "Diarizing",
      }),
    ).toBeInTheDocument();

    finish();
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("Finished"),
    );
  });

  it("shows the persisted transcript as diarization continues", async () => {
    arrangeLibrary();
    deferTranscription();
    let phaseHandler: (p: {
      id: number;
      phase: "diarizing";
    }) => void = () => {};
    vi.mocked(ipc.onTranscriptionPhase).mockImplementation(async (handler) => {
      phaseHandler = handler;
      return () => {};
    });
    const user = userEvent.setup();
    render(<App />);

    await startTranscribing(user);
    vi.mocked(ipc.openMeeting).mockImplementation(async (id) => {
      if (id === READY_ACTIVE.id) return { ...ACTIVE_FINISHED };
      const found = LIBRARY.find((meeting) => meeting.id === id);
      if (!found) throw new Error(`no meeting ${id}`);
      return { ...found };
    });

    phaseHandler({ id: READY_ACTIVE.id, phase: "diarizing" });

    expect(await screen.findByDisplayValue("Hello")).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Diarizing");
  });

  it("ignores a phase event that arrives after the run it belongs to has already finished", async () => {
    // Guards against a delayed `transcription_phase` event outliving its own
    // run: if it arrived after the meeting's run cleared (`transcribingId`
    // back to null), it must not resurrect a "Diarizing" tone on a meeting
    // that has already finished.
    arrangeLibrary();
    const finish = deferTranscription();
    let phaseHandler: (p: {
      id: number;
      phase: "diarizing";
    }) => void = () => {};
    vi.mocked(ipc.onTranscriptionPhase).mockImplementation(async (handler) => {
      phaseHandler = handler;
      return () => {};
    });
    const user = userEvent.setup();
    render(<App />);

    await startTranscribing(user);
    finish();
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("Finished"),
    );

    // The stale event, from the run that just ended, arrives late.
    phaseHandler({ id: READY_ACTIVE.id, phase: "diarizing" });

    expect(screen.getByRole("status")).toHaveTextContent("Finished");
    expect(
      within(row(READY_ACTIVE.title)).queryByRole("img", {
        name: "Diarizing",
      }),
    ).not.toBeInTheDocument();
  });

  it("ignores a phase event for a meeting that is not the one running", async () => {
    arrangeLibrary();
    deferTranscription();
    let phaseHandler: (p: {
      id: number;
      phase: "diarizing";
    }) => void = () => {};
    vi.mocked(ipc.onTranscriptionPhase).mockImplementation(async (handler) => {
      phaseHandler = handler;
      return () => {};
    });
    const user = userEvent.setup();
    render(<App />);

    await startTranscribing(user);
    phaseHandler({ id: READY_OTHER.id, phase: "diarizing" });

    expect(screen.getByRole("status")).toHaveTextContent("Transcribing");
  });

  it("reports the error tone and restores the row actions when transcription fails", async () => {
    arrangeLibrary();
    vi.mocked(ipc.transcribeMeeting).mockRejectedValue(
      new Error("decode blew up"),
    );
    const user = userEvent.setup();
    render(<App />);

    await startTranscribing(user);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "decode blew up",
    );
    const header = screen.getByRole("status");
    expect(within(header).getByText("Error")).toHaveClass("wp-tone--error");

    // The run is over: the row drops the spinner and gets its actions back.
    const active = row(READY_ACTIVE.title);
    expect(
      within(active).queryByRole("img", { name: "Transcribing" }),
    ).not.toBeInTheDocument();
    expect(
      within(active).getByRole("button", {
        name: `Rename ${READY_ACTIVE.title}`,
      }),
    ).toBeInTheDocument();
  });
});

describe("App — a run that ends after the user has moved on", () => {
  // These cover the negative branch of the "is this meeting still on screen?"
  // guard: a run that finishes — or fails — while a *different* meeting is
  // open must not reach across and rewrite what the user is looking at.
  const RUNNER: Meeting = {
    id: 1,
    title: "Runner meeting",
    created_at_ms: 2_000,
    language: "ru",
    status: "ready",
    source_path: "/path/to/meeting.mp3",
    source_name: "meeting.mp3",
    segments: [],
    source_missing: false,
  };

  const BYSTANDER: Meeting = {
    id: 2,
    title: "Bystander meeting",
    created_at_ms: 1_000,
    language: "ru",
    status: "no_files",
    segments: [],
    source_missing: false,
  };

  const RUNNER_FINISHED: Meeting = {
    ...RUNNER,
    status: "finished",
    duration_ms: 1_000,
    segments: [HELLO_SEGMENT],
  };

  const LIBRARY = [RUNNER, BYSTANDER];

  function arrange() {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue(
      LIBRARY.map((m) => ({
        id: m.id,
        title: m.title,
        created_at_ms: m.created_at_ms,
        duration_ms: m.duration_ms,
        status: m.status,
      })),
    );
    vi.mocked(ipc.openMeeting).mockImplementation(async (id) => {
      const found = LIBRARY.find((m) => m.id === id);
      if (!found) throw new Error(`no meeting ${id}`);
      return { ...found };
    });
  }

  /** Start the run on RUNNER, then open BYSTANDER while it is still going. */
  async function startRunThenLeave(
    user: ReturnType<typeof userEvent.setup>,
  ): Promise<void> {
    await screen.findByRole("heading", { name: RUNNER.title });
    const transcribe = screen.getByRole("button", { name: "Transcribe" });
    await waitFor(() => expect(transcribe).not.toBeDisabled());
    await user.click(transcribe);
    await within(
      screen.getByRole("listitem", { name: RUNNER.title }),
    ).findByRole("img", { name: "Transcribing" });
    await user.click(
      screen.getByRole("button", { name: `Open ${BYSTANDER.title}` }),
    );
    await screen.findByRole("heading", { name: BYSTANDER.title });
  }

  it("does not pull the workspace back when the run succeeds elsewhere", async () => {
    arrange();
    let resolveTranscribe: (result: TranscribeMeetingResult) => void = () => {};
    vi.mocked(ipc.transcribeMeeting).mockReturnValue(
      new Promise<TranscribeMeetingResult>((resolve) => {
        resolveTranscribe = resolve;
      }),
    );
    const user = userEvent.setup();
    render(<App />);

    await startRunThenLeave(user);
    resolveTranscribe(transcribeResult({ ...RUNNER_FINISHED }));

    // The sidebar summary still updates — only the workspace is left alone.
    await waitFor(() =>
      expect(
        within(screen.getByRole("listitem", { name: RUNNER.title })).getByRole(
          "img",
          { name: "Finished" },
        ),
      ).toBeInTheDocument(),
    );
    expect(
      screen.getByRole("heading", { name: BYSTANDER.title }),
    ).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("No files");
    expect(screen.queryByDisplayValue("Hello")).not.toBeInTheDocument();
  });

  it("does not paint a failed run onto the meeting the user moved to", async () => {
    arrange();
    let rejectTranscribe: (reason: Error) => void = () => {};
    vi.mocked(ipc.transcribeMeeting).mockReturnValue(
      new Promise<TranscribeMeetingResult>((_resolve, reject) => {
        rejectTranscribe = reject;
      }),
    );
    const user = userEvent.setup();
    render(<App />);

    await startRunThenLeave(user);
    rejectTranscribe(new Error("decode blew up"));

    // The spinner must clear, proving the rejection was actually handled...
    await waitFor(() =>
      expect(
        within(
          screen.getByRole("listitem", { name: RUNNER.title }),
        ).queryByRole("img", { name: "Transcribing" }),
      ).not.toBeInTheDocument(),
    );
    // ...but the bystander must show no trace of a failure it never had.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    const header = screen.getByRole("status");
    expect(header).toHaveTextContent("No files");
    expect(within(header).queryByText("Error")).not.toBeInTheDocument();
  });
});

describe("App — controls unrelated to the running meeting", () => {
  const RUNNER: Meeting = {
    id: 1,
    title: "Runner meeting",
    created_at_ms: 2_000,
    language: "ru",
    status: "ready",
    source_path: "/path/to/meeting.mp3",
    source_name: "meeting.mp3",
    segments: [],
    source_missing: false,
  };

  const DONE: Meeting = {
    id: 2,
    title: "Done meeting",
    created_at_ms: 1_000,
    duration_ms: 1_000,
    language: "ru",
    status: "finished",
    source_path: "/path/to/other.mp3",
    source_name: "other.mp3",
    segments: [HELLO_SEGMENT],
    source_missing: false,
  };

  it("leaves Save and Choose file usable on a meeting that is not the one running", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue(
      [RUNNER, DONE].map((m) => ({
        id: m.id,
        title: m.title,
        created_at_ms: m.created_at_ms,
        duration_ms: m.duration_ms,
        status: m.status,
      })),
    );
    vi.mocked(ipc.openMeeting).mockImplementation(async (id) =>
      id === DONE.id ? { ...DONE } : { ...RUNNER },
    );
    vi.mocked(ipc.transcribeMeeting).mockReturnValue(
      new Promise<TranscribeMeetingResult>(() => {}),
    );
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: RUNNER.title });
    const transcribe = screen.getByRole("button", { name: "Transcribe" });
    await waitFor(() => expect(transcribe).not.toBeDisabled());
    await user.click(transcribe);

    await user.click(
      screen.getByRole("button", { name: `Open ${DONE.title}` }),
    );
    await screen.findByRole("heading", { name: DONE.title });

    // Only Transcribe is globally blocked (one run at a time). Exporting an
    // unrelated finished transcript, or attaching a file to another meeting,
    // has nothing to do with the run in flight.
    expect(screen.getByRole("button", { name: "Transcribe" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).not.toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Choose file" }),
    ).not.toBeDisabled();
    // Renaming or deleting a meeting that is not the one running is likewise
    // none of the run's business — and this matches the sidebar row's rule.
    expect(
      screen.getByRole("button", { name: "Rename meeting" }),
    ).not.toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Delete meeting" }),
    ).not.toBeDisabled();
  });
});

describe("App — transcript panel while its own meeting is transcribing", () => {
  // Re-running transcription on a meeting that already has a transcript: the
  // stale segments come back onto screen if the user leaves and reopens this
  // meeting before the run finishes (`openMeeting` still returns the old,
  // not-yet-overwritten record). They must not be left editable underneath
  // the in-flight run.
  const RUNNER: Meeting = {
    id: 1,
    title: "Runner meeting",
    created_at_ms: 2_000,
    language: "ru",
    status: "finished",
    duration_ms: 1_000,
    source_path: "/path/to/meeting.mp3",
    source_name: "meeting.mp3",
    segments: [HELLO_SEGMENT],
    source_missing: false,
  };

  const OTHER: Meeting = {
    id: 2,
    title: "Other meeting",
    created_at_ms: 1_000,
    language: "ru",
    status: "no_files",
    segments: [],
    source_missing: false,
  };

  it("disables the existing segments and speaker names once the meeting is reopened mid-run", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue(
      [RUNNER, OTHER].map((m) => ({
        id: m.id,
        title: m.title,
        created_at_ms: m.created_at_ms,
        duration_ms: m.duration_ms,
        status: m.status,
      })),
    );
    vi.mocked(ipc.openMeeting).mockImplementation(async (id) =>
      id === OTHER.id ? { ...OTHER } : { ...RUNNER },
    );
    vi.mocked(ipc.transcribeMeeting).mockReturnValue(
      new Promise<TranscribeMeetingResult>(() => {}),
    );
    const user = userEvent.setup();
    render(<App />);

    await screen.findByDisplayValue("Hello");
    const transcribe = screen.getByRole("button", { name: "Transcribe" });
    await waitFor(() => expect(transcribe).not.toBeDisabled());
    await user.click(transcribe);

    // Leave, then come back while the run on RUNNER is still going.
    await user.click(
      screen.getByRole("button", { name: `Open ${OTHER.title}` }),
    );
    await screen.findByRole("heading", { name: OTHER.title });
    await user.click(
      screen.getByRole("button", { name: `Open ${RUNNER.title}` }),
    );
    await screen.findByRole("heading", { name: RUNNER.title });

    // The stale transcript is back on screen, but locked.
    expect(await screen.findByDisplayValue("Hello")).toBeDisabled();
  });
});

// Every IPC call the workspace makes can fail. None of these failures may lose
// the workspace: the app reports what went wrong and stays usable.
describe("App — IPC failures surface as errors without breaking the workspace", () => {
  const SAVED_MEETING: Meeting = {
    id: 7,
    title: "Quarterly planning",
    created_at_ms: 2_000,
    language: "ru",
    status: "finished",
    segments: [{ start_ms: 0, end_ms: 1_000, text: "Saved transcript" }],
    source_missing: false,
  };

  function arrangeSavedMeeting() {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: SAVED_MEETING.id,
        title: SAVED_MEETING.title,
        created_at_ms: SAVED_MEETING.created_at_ms,
        status: SAVED_MEETING.status,
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue(SAVED_MEETING);
  }

  it("reports a library that cannot be read instead of rendering an empty workspace", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockRejectedValue(
      new Error("library unreadable"),
    );
    render(<App />);

    expect(await screen.findByText(/library unreadable/)).toBeInTheDocument();
  });

  it("reports a failure to attach the chosen file", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.setMeetingSource).mockRejectedValue(
      new Error("cannot attach that file"),
    );
    const user = userEvent.setup();
    render(<App />);
    await waitForAddFileEnabled();

    await user.click(screen.getByRole("button", { name: "Choose file" }));

    expect(
      await screen.findByText(/cannot attach that file/),
    ).toBeInTheDocument();
  });

  it("reports a failure to remove the attached file and keeps the file attached", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    const user = userEvent.setup();
    render(<App />);
    await waitForAddFileEnabled();
    await user.click(screen.getByRole("button", { name: "Choose file" }));
    await screen.findByRole("button", { name: "Remove file" });

    vi.mocked(ipc.setMeetingSource).mockRejectedValue(
      new Error("cannot detach"),
    );
    await user.click(screen.getByRole("button", { name: "Remove file" }));

    expect(await screen.findByText(/cannot detach/)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Remove file" }),
    ).toBeInTheDocument();
  });

  it("reports a rename that the store rejects", async () => {
    arrangeSavedMeeting();
    vi.mocked(ipc.renameMeeting).mockRejectedValue(
      new Error("rename rejected by store"),
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("heading", { name: SAVED_MEETING.title });

    await user.click(screen.getByRole("button", { name: "Rename meeting" }));
    const dialog = screen.getByRole("dialog", { name: "Rename meeting" });
    const input = within(dialog).getByRole("textbox", {
      name: "Meeting label",
    });
    await user.clear(input);
    await user.type(input, "Renamed");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(
      await screen.findByText(/rename rejected by store/),
    ).toBeInTheDocument();
  });

  it("reports a delete that the store rejects and keeps the meeting", async () => {
    arrangeSavedMeeting();
    vi.mocked(ipc.deleteMeeting).mockRejectedValue(
      new Error("delete rejected by store"),
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("heading", { name: SAVED_MEETING.title });

    await user.click(screen.getByRole("button", { name: "Delete meeting" }));
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Delete",
      }),
    );

    expect(
      await screen.findByText(/delete rejected by store/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: SAVED_MEETING.title }),
    ).toBeInTheDocument();
  });

  // The workspace is always backed by a real meeting, so deleting the last one
  // must seed a replacement rather than leave the user with nothing to open.
  it("seeds a fresh meeting when the last remaining one is deleted", async () => {
    arrangeSavedMeeting();
    vi.mocked(ipc.deleteMeeting).mockResolvedValue(undefined);
    vi.mocked(ipc.createMeeting).mockResolvedValue({
      ...EMPTY_MEETING,
      id: 900,
      title: "Fresh start",
    });
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("heading", { name: SAVED_MEETING.title });

    await user.click(screen.getByRole("button", { name: "Delete meeting" }));
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Delete",
      }),
    );

    expect(
      await screen.findByRole("heading", { name: "Fresh start" }),
    ).toBeInTheDocument();
    expect(ipc.createMeeting).toHaveBeenCalled();
  });

  it("closes the delete confirmation on Escape without deleting", async () => {
    arrangeSavedMeeting();
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("heading", { name: SAVED_MEETING.title });

    await user.click(screen.getByRole("button", { name: "Delete meeting" }));
    const dialog = screen.getByRole("alertdialog");
    // Escape is handled on the panel, and this dialog does not move focus into
    // itself when it opens, so it only reaches the handler once focus is inside
    // — the keyboard path a user actually takes after tabbing in.
    within(dialog).getByRole("button", { name: "Cancel" }).focus();
    await user.keyboard("{Escape}");

    await waitFor(() =>
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument(),
    );
    expect(ipc.deleteMeeting).not.toHaveBeenCalled();
  });

  it("closes the speaker-identification warning on Escape", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.transcribeMeeting).mockResolvedValue(
      transcribeResult(
        transcribedMeeting([HELLO_SEGMENT]),
        "Speaker identification is unavailable: engine exploded",
      ),
    );
    const user = userEvent.setup();
    render(<App />);
    await waitForAddFileEnabled();

    await chooseAndTranscribe(user);
    const warning = await screen.findByRole("alertdialog", {
      name: "Speaker identification issue",
    });
    expect(warning).toHaveTextContent(/engine exploded/);
    await user.keyboard("{Escape}");

    await waitFor(() =>
      expect(
        screen.queryByRole("alertdialog", {
          name: "Speaker identification issue",
        }),
      ).not.toBeInTheDocument(),
    );
  });

  // Durations cross into hours for real meetings; the sidebar switches format
  // rather than showing a minute count that keeps growing.
  it("formats a sidebar duration of an hour or more as hours and minutes", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: SAVED_MEETING.id,
        title: SAVED_MEETING.title,
        created_at_ms: SAVED_MEETING.created_at_ms,
        duration_ms: 3_723_000,
        status: "finished",
      },
    ]);
    vi.mocked(ipc.openMeeting).mockResolvedValue(SAVED_MEETING);
    render(<App />);

    expect(await screen.findByText("1h 02m")).toBeInTheDocument();
  });
});

describe("App — Streaming mode toggle", () => {
  it("opens the Streaming view from the sidebar toggle and returns to Meetings", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    render(<App />);
    await waitForAddFileEnabled();

    await user.click(screen.getByRole("button", { name: "Streaming" }));

    expect(
      await screen.findByRole("heading", { name: "Streaming Session" }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Meeting" }));

    expect(
      await screen.findByRole("button", { name: "Choose file" }),
    ).toBeInTheDocument();
  });
});
