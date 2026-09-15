import { mockCreateIpc } from "./test/ipcMock";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import * as ipc from "./ipc";
import type { Meeting } from "./ipc";
import {
  TRANSCRIPTION_DOWNLOADED,
  EMPTY_MEETING,
  HELLO_SEGMENT,
  transcribedMeeting,
  transcribeResult,
  waitForAddFileEnabled,
  chooseAndTranscribe,
  resetAppMocks,
} from "./test/appTestHarness";

vi.mock("./ipc", () => mockCreateIpc());

beforeEach(resetAppMocks);

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

  it("[WP-130] reports an export failure in the workspace instead of rejecting the click", async () => {
    arrangeSavedMeeting();
    vi.mocked(ipc.saveTextDialog).mockRejectedValue(new Error("export denied"));
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: SAVED_MEETING.title });
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("export denied");
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

    await user.click(
      screen.getByRole("button", { name: "Rename transcription" }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Rename transcription",
    });
    const input = within(dialog).getByRole("textbox", {
      name: "Transcription title",
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

    await user.click(
      screen.getByRole("button", { name: "Delete transcription" }),
    );
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

  it("allows retrying a delete after the store rejects the first attempt", async () => {
    arrangeSavedMeeting();
    vi.mocked(ipc.deleteMeeting)
      .mockRejectedValueOnce(new Error("delete rejected by store"))
      .mockResolvedValueOnce(undefined);
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("heading", { name: SAVED_MEETING.title });

    await user.click(
      screen.getByRole("button", { name: "Delete transcription" }),
    );
    const dialog = screen.getByRole("alertdialog");
    const confirm = within(dialog).getByRole("button", { name: "Delete" });
    await user.click(confirm);

    await screen.findByText(/delete rejected by store/);
    await waitFor(() => expect(confirm).not.toBeDisabled());
    await user.click(confirm);

    await waitFor(() => expect(ipc.deleteMeeting).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
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

    await user.click(
      screen.getByRole("button", { name: "Delete transcription" }),
    );
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

  it("removes a deleted transcription from the sidebar and closes its dialog when opening the replacement fails", async () => {
    const replacement: Meeting = {
      ...SAVED_MEETING,
      id: 8,
      title: "Replacement transcription",
      created_at_ms: 1_000,
    };
    vi.mocked(ipc.listMeetings).mockResolvedValue([
      {
        id: SAVED_MEETING.id,
        title: SAVED_MEETING.title,
        created_at_ms: SAVED_MEETING.created_at_ms,
        status: SAVED_MEETING.status,
      },
      {
        id: replacement.id,
        title: replacement.title,
        created_at_ms: replacement.created_at_ms,
        status: replacement.status,
      },
    ]);
    vi.mocked(ipc.openMeeting)
      .mockResolvedValueOnce(SAVED_MEETING)
      .mockRejectedValueOnce(new Error("replacement cannot open"));
    vi.mocked(ipc.deleteMeeting).mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("heading", { name: SAVED_MEETING.title });

    await user.click(
      screen.getByRole("button", { name: "Delete transcription" }),
    );
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Delete",
      }),
    );

    expect(
      await screen.findByText(/replacement cannot open/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: `Open ${SAVED_MEETING.title}` }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Open ${replacement.title}` }),
    ).toBeInTheDocument();
  });

  it("closes the delete dialog and removes the last transcription even when seeding fails", async () => {
    arrangeSavedMeeting();
    vi.mocked(ipc.deleteMeeting).mockResolvedValue(undefined);
    vi.mocked(ipc.createMeeting).mockRejectedValue(
      new Error("replacement cannot be created"),
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("heading", { name: SAVED_MEETING.title });

    await user.click(
      screen.getByRole("button", { name: "Delete transcription" }),
    );
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Delete",
      }),
    );

    expect(
      await screen.findByText(/replacement cannot be created/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: `Open ${SAVED_MEETING.title}` }),
    ).not.toBeInTheDocument();
  });

  it("closes the delete confirmation on Escape without deleting", async () => {
    arrangeSavedMeeting();
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("heading", { name: SAVED_MEETING.title });

    await user.click(
      screen.getByRole("button", { name: "Delete transcription" }),
    );
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
