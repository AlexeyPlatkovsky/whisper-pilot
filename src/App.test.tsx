import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
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
  createMeeting: vi.fn(),
  deleteMeeting: vi.fn(),
  listMeetings: vi.fn(),
  openMeeting: vi.fn(),
  renameMeeting: vi.fn(),
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
  vi.mocked(ipc.getSettings).mockResolvedValue({
    theme: "system",
    ui_language: "en",
    active_model_diarization: "none",
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
});

describe("App — theme application", () => {
  it("applies the persisted theme on mount, before the user ever opens Settings", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "dark",
      ui_language: "en",
      active_model_diarization: "none",
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

describe("App — persisted meeting workspace", () => {
  const NEWEST_MEETING = {
    id: 2,
    title: "Newest meeting",
    created_at_ms: 2_000,
    duration_ms: 1_000,
    language: "ru",
    status: "finished",
    segments: [{ start_ms: 0, end_ms: 1_000, text: "Saved transcript" }],
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
    });
    vi.mocked(ipc.createMeeting).mockResolvedValue({
      id: 3,
      title: "New Meeting",
      created_at_ms: 3_000,
      language: "ru",
      status: "no_files",
      segments: [],
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

describe("App — persisted meeting controls", () => {
  const ACTIVE_MEETING = {
    id: 2,
    title: "Quarterly planning",
    created_at_ms: 2_000,
    language: "ru",
    status: "finished",
    segments: [{ start_ms: 0, end_ms: 1_000, text: "Saved transcript" }],
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
  };

  const NO_FILES: Meeting = {
    id: 4,
    title: "Empty meeting",
    created_at_ms: 1_000,
    language: "ru",
    status: "no_files",
    segments: [],
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
  };

  const BYSTANDER: Meeting = {
    id: 2,
    title: "Bystander meeting",
    created_at_ms: 1_000,
    language: "ru",
    status: "no_files",
    segments: [],
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
