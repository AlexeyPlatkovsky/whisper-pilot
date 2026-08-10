import { mockCreateIpc } from "./test/ipcMock";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import * as ipc from "./ipc";
import type { Meeting } from "./ipc";
import {
  TRANSCRIPTION_DOWNLOADED,
  ATTACHED_MEETING,
  HELLO_SEGMENT,
  transcribedMeeting,
  resetAppMocks,
} from "./test/appTestHarness";

vi.mock("./ipc", () => mockCreateIpc());

beforeEach(resetAppMocks);

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
    // Real timers, not fake: the toast's async continuation (await writeText →
    // setCopied) and its 2500ms rollback timer are verified with waitFor, which
    // is deterministic under CI load — fake timers with shouldAdvanceTime fold
    // real wall-clock into the fake clock and can roll the toast back before
    // an assertion observes it on a slow runner.
    const user = userEvent.setup();
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

    const toast = await screen.findByText("Copied", { selector: ".wp-toast" });
    expect(toast).toHaveAttribute("role", "status");
    expect(toast).toHaveClass("wp-toast--top");
    expect(screen.getByRole("button", { name: "Copied" })).toBeInTheDocument();

    // The 2500ms rollback timer is real, so wait for it to fire.
    await waitFor(
      () => {
        expect(
          screen.queryByText("Copied", { selector: ".wp-toast" }),
        ).not.toBeInTheDocument();
      },
      { timeout: 4000 },
    );
    expect(
      screen.getByRole("button", { name: "Copy transcript" }),
    ).toBeInTheDocument();
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
    // Real timers, same reason as the first toast test: the toast's async
    // continuation and its rollback timer are verified with waitFor, which is
    // deterministic under CI load instead of racing fake-timer advancement.
    const user = userEvent.setup();
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
    await screen.findByText("Copied", { selector: ".wp-toast" });

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

    // The cancelled rollback timer must not resurrect the feedback on the new
    // meeting — wait longer than the 2500ms window and re-check.
    await waitFor(
      () => {
        expect(
          screen.queryByText("Copied", { selector: ".wp-toast" }),
        ).not.toBeInTheDocument();
      },
      { timeout: 4000 },
    );
    expect(
      screen.getByRole("button", { name: "Copy transcript" }),
    ).toBeInTheDocument();
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
