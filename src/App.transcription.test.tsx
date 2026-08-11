import { mockCreateIpc } from "./test/ipcMock";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import * as ipc from "./ipc";
import type { Meeting, TranscribeMeetingResult } from "./ipc";
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

describe("App — transcription without Stop", () => {
  it("does not offer Stop while a Meeting transcription is running", async () => {
    vi.mocked(ipc.listTaskModels).mockResolvedValue([TRANSCRIPTION_DOWNLOADED]);
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

    expect(
      screen.queryByRole("button", { name: "Stop" }),
    ).not.toBeInTheDocument();
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
