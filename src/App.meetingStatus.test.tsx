import { mockCreateIpc } from "./test/ipcMock";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import * as ipc from "./ipc";
import type { Meeting, TranscribeMeetingResult } from "./ipc";
import { resolveMeetingStatus } from "./meetingStatus";
import {
  TRANSCRIPTION_DOWNLOADED,
  HELLO_SEGMENT,
  transcribeResult,
  resetAppMocks,
} from "./test/appTestHarness";

vi.mock("./ipc", () => mockCreateIpc());

beforeEach(resetAppMocks);

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
