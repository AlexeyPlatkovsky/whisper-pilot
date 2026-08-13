import { describe, expect, it } from "vitest";
import {
  exportFileExtension,
  renderForExport,
  renderMarkdown,
  renderPlainText,
} from "./export";
import type { MeetingMfu, Segment } from "./ipc";

const SEGMENTS: Segment[] = [
  { start_ms: 0, end_ms: 1_000, text: "Hello" },
  { start_ms: 65_000, end_ms: 70_000, text: "there", speaker_id: 2 },
];

const MFU: MeetingMfu = {
  meeting_id: 1,
  summary: "Summary text",
  decisions: "",
  action_items: "Do the thing",
  open_questions: "",
  participants: "Alice",
};

const label = (id: number) => `Speaker ${id + 1}`;

describe("renderPlainText", () => {
  it("preserves today's existing rendering: speaker-prefixed lines, no headers, no mfu", () => {
    const text = renderPlainText(SEGMENTS, label);

    expect(text).toBe("Hello\nSpeaker 3: there");
  });

  it("renders an empty transcript as an empty string", () => {
    expect(renderPlainText([], label)).toBe("");
  });
});

describe("renderMarkdown", () => {
  it("renders a transcript heading, bold speaker labels, and bracketed m:ss timestamps", () => {
    const text = renderMarkdown(SEGMENTS, null, label);

    expect(text).toBe(
      ["# Transcript", "", "[0:00] Hello", "**Speaker 3** [1:05]: there"].join(
        "\n",
      ),
    );
  });

  it("appends only the mfu sections that have content, each under its own heading", () => {
    const text = renderMarkdown(SEGMENTS, MFU, label);

    expect(text).toContain("## MFU");
    expect(text).toContain("### Summary\n\nSummary text");
    expect(text).toContain("### Action Items\n\nDo the thing");
    expect(text).toContain("### Participants\n\nAlice");
    expect(text).not.toContain("### Decisions");
    expect(text).not.toContain("### Open Questions");
  });

  it("omits the MFU section entirely when mfu is null", () => {
    expect(renderMarkdown(SEGMENTS, null, label)).not.toContain("## MFU");
  });
});

describe("renderForExport", () => {
  it("dispatches to plain text or markdown by file type", () => {
    expect(renderForExport("plain_text", SEGMENTS, MFU, label)).toBe(
      renderPlainText(SEGMENTS, label),
    );
    expect(renderForExport("markdown", SEGMENTS, MFU, label)).toBe(
      renderMarkdown(SEGMENTS, MFU, label),
    );
  });
});

describe("exportFileExtension", () => {
  it("maps plain_text to txt and markdown to md", () => {
    expect(exportFileExtension("plain_text")).toBe("txt");
    expect(exportFileExtension("markdown")).toBe("md");
  });
});
