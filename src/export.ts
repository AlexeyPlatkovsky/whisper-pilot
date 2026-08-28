import type {
  MeetingMfu,
  Segment,
  StreamingTranslationTargetLanguage,
  StreamingWindow,
} from "./ipc";
import {
  paragraphTranslatedText,
  plainTranscript,
  type TranslationEntry,
} from "./streamingText";

export type ExportFileType = "plain_text" | "markdown";

function formatTimestamp(ms: number): string {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** Today's existing rendering — transcript only, no MFU, no headers. */
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

const MFU_SECTIONS: { heading: string; field: keyof MeetingMfu }[] = [
  { heading: "Summary", field: "summary" },
  { heading: "Decisions", field: "decisions" },
  { heading: "Action Items", field: "action_items" },
  { heading: "Open Questions", field: "open_questions" },
  { heading: "Participants", field: "participants" },
];

/** Transcript and (when present) MFU, Markdown-formatted (WP-15): headers,
 * bold speaker labels, and bracketed m:ss timestamps. */
export function renderMarkdown(
  segments: Segment[],
  mfu: MeetingMfu | null,
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

  if (mfu) {
    lines.push("", "## MFU");
    for (const { heading, field } of MFU_SECTIONS) {
      const value = mfu[field];
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
  mfu: MeetingMfu | null,
  resolveSpeakerLabel: (id: number) => string,
): string {
  return fileType === "markdown"
    ? renderMarkdown(segments, mfu, resolveSpeakerLabel)
    : renderPlainText(segments, resolveSpeakerLabel);
}

export function exportFileExtension(fileType: ExportFileType): string {
  return fileType === "markdown" ? "md" : "txt";
}

// --- WP-94: Streaming Copy/Export paired original+translation rendering ---
// Scoped to Streaming only — Meeting's export above is untouched. Kept
// decoupled from StreamingView's component state (a narrow structural type
// instead of importing its TranslationEntry) so the renderer is unit-
// testable without rendering; StreamingView.tsx's translations Map is
// already structurally compatible and is passed straight through.

/** Display names for the translation target languages — the single source
 * for the Streaming select, the split grid's target-column header, and the
 * exported block labels. */
export const STREAMING_TARGET_LANGUAGE_NAMES: Record<
  StreamingTranslationTargetLanguage,
  string
> = {
  en: "English",
  ru: "Русский",
};

/** One paragraph as shown on the Streaming screen — its windows, in
 * on-screen order (WP-103: translation is keyed per window, not per
 * paragraph, so this is what `renderStreamingPaired` needs to independently
 * derive both the paragraph's original text and its translated text). */
export type StreamingExportParagraph = StreamingWindow[];

/** Whether Copy/Export should switch into the paired original+translation
 * rendering at all — true once at least one window has a translation entry.
 * False (Live Translation off, or on with nothing recorded yet) means
 * Copy/Export stay on today's single-column rendering, unchanged (WP-94). */
export function hasStreamingTranslations(
  translations: Map<number, TranslationEntry>,
): boolean {
  return translations.size > 0;
}

/**
 * Pairs each paragraph's original text with its translation for Streaming's
 * Copy/Export (WP-94, per-window since WP-103), in on-screen order.
 * "Export what the screen shows" — each translated block comes from
 * `streamingText.ts`'s `paragraphTranslatedText`, so a partially-translated
 * paragraph shows real text for its finished windows and a placeholder only
 * for the unfinished tail, and the two sides never drift out of count.
 */
export function renderStreamingPaired(
  paragraphs: StreamingExportParagraph[],
  translations: Map<number, TranslationEntry>,
  targetLanguage: StreamingTranslationTargetLanguage,
): string {
  const targetLabel = STREAMING_TARGET_LANGUAGE_NAMES[targetLanguage];
  const blocks = paragraphs.map((windows) => {
    const sourceText = plainTranscript(windows);
    const translatedText = paragraphTranslatedText(windows, translations);
    return `Original:\n${sourceText}\n\n${targetLabel}:\n${translatedText}`;
  });
  return blocks.join("\n\n");
}
