import { mockCreateIpc } from "./test/ipcMock";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import * as ipc from "./ipc";
import {
  TRANSCRIPTION_DOWNLOADED,
  TRANSCRIPTION_NOT_DOWNLOADED,
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
