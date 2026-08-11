import type { MeetingNotes, Segment } from "./ipc";

export type ExportFileType = "plain_text" | "markdown";

function formatTimestamp(ms: number): string {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** Today's existing rendering — transcript only, no notes, no headers. */
export function renderPlainText(
  segments: Segment[],
  resolveSpeakerLabel: (id: number) => string,
): string {
  return segments
    .map((s) =>
      s.speaker_id !== undefined
        ? `${resolveSpeakerLabel(s.speaker_id)}: ${s.text}`
        : s.text,
    )
    .join("\n");
}

const NOTES_SECTIONS: { heading: string; field: keyof MeetingNotes }[] = [
  { heading: "Summary", field: "summary" },
  { heading: "Decisions", field: "decisions" },
  { heading: "Action Items", field: "action_items" },
  { heading: "Open Questions", field: "open_questions" },
  { heading: "Participants", field: "participants" },
];

/** Transcript and (when present) notes, Markdown-formatted (WP-15): headers,
 * bold speaker labels, and bracketed m:ss timestamps. */
export function renderMarkdown(
  segments: Segment[],
  notes: MeetingNotes | null,
  resolveSpeakerLabel: (id: number) => string,
): string {
  const lines: string[] = ["# Transcript", ""];
  for (const s of segments) {
    const timestamp = `[${formatTimestamp(s.start_ms)}]`;
    lines.push(
      s.speaker_id !== undefined
        ? `**${resolveSpeakerLabel(s.speaker_id)}** ${timestamp}: ${s.text}`
        : `${timestamp} ${s.text}`,
    );
  }

  if (notes) {
    lines.push("", "## Notes");
    for (const { heading, field } of NOTES_SECTIONS) {
      const value = notes[field];
      if (typeof value === "string" && value) {
        lines.push("", `### ${heading}`, "", value);
      }
    }
  }

  return lines.join("\n");
}

/** Renders a meeting for export/copy according to the selected file type —
 * the single place both the file-save and header-copy actions call, so they
 * can never drift apart (WP-15). */
export function renderForExport(
  fileType: ExportFileType,
  segments: Segment[],
  notes: MeetingNotes | null,
  resolveSpeakerLabel: (id: number) => string,
): string {
  return fileType === "markdown"
    ? renderMarkdown(segments, notes, resolveSpeakerLabel)
    : renderPlainText(segments, resolveSpeakerLabel);
}

export function exportFileExtension(fileType: ExportFileType): string {
  return fileType === "markdown" ? "md" : "txt";
}
