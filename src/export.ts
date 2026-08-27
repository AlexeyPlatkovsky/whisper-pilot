import type {
  MeetingMfu,
  Segment,
  StreamingTranslationTargetLanguage,
} from "./ipc";

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

export type StreamingTranslationStatus =
  "pending" | "translating" | "done" | "mirrored" | "failed";

/** One paragraph's Live Translation entry, structurally identical to
 * StreamingView's `TranslationEntry` (WP-93). */
export interface StreamingTranslationEntry {
  status: StreamingTranslationStatus;
  sourceText: string;
  translatedText?: string;
}

/** One paragraph as shown on the Streaming screen: `key` is its
 * `paragraph_key` (the `window_index` of the paragraph's first window),
 * `sourceText` its current on-screen original text. */
export interface StreamingExportParagraph {
  key: number;
  sourceText: string;
}

/** Placeholder for a paragraph with no usable translation yet — missing,
 * still pending/translating, or one whose stored source text no longer
 * matches the paragraph's current text (stale) — used uniformly across
 * those causes so the original and translated sides always have the same
 * paragraph count (WP-94). A *mirrored* paragraph is not one of these
 * causes — see `STREAMING_TRANSLATION_FAILED_PLACEHOLDER` and
 * `renderStreamingPaired` below for why mirrored and failed are handled
 * differently. */
export const STREAMING_TRANSLATION_PLACEHOLDER = "[Not translated]";

/** Placeholder for a paragraph whose translation ran and errored. Distinct
 * wording from `STREAMING_TRANSLATION_PLACEHOLDER` because a failure needs a
 * manual retry rather than more waiting. See WP-94. */
export const STREAMING_TRANSLATION_FAILED_PLACEHOLDER = "[Translation failed]";

/** Whether Copy/Export should switch into the paired original+translation
 * rendering at all — true once at least one paragraph has a translation
 * entry. False (Live Translation off, or on with nothing recorded yet)
 * means Copy/Export stay on today's single-column rendering, unchanged
 * (WP-94). */
export function hasStreamingTranslations(
  translations: Map<number, StreamingTranslationEntry>,
): boolean {
  return translations.size > 0;
}

/**
 * Pairs each paragraph's original text with its translation for Streaming's
 * Copy/Export (WP-94): one "Original" block followed by one target-language
 * block per paragraph, in the same order as the on-screen grid
 * (`groupWindowsIntoParagraphs` order). The rule is "export what the screen
 * shows":
 *  - `"done"` with its stored source text still matching the paragraph's
 *    current text renders the real translated text.
 *  - `"mirrored"` (with matching source text) renders the paragraph's own
 *    text — the same text the on-screen right cell shows in its muted
 *    style. This is not a failure: the paragraph's windows are already
 *    entirely in the target language, so no model call was made by design.
 *  - `"failed"` (with matching source text) renders
 *    `STREAMING_TRANSLATION_FAILED_PLACEHOLDER` — distinct wording from the
 *    not-yet-translated case, since a failed attempt already ran and needs
 *    a manual retry rather than just more waiting.
 *  - everything else — missing entry, `"pending"`, `"translating"`, or a
 *    stored source text that no longer matches the paragraph's current text
 *    (stale, regardless of status) — renders
 *    `STREAMING_TRANSLATION_PLACEHOLDER` instead of being dropped or left
 *    blank, so the two sides can never drift out of paragraph count.
 */
export function renderStreamingPaired(
  paragraphs: StreamingExportParagraph[],
  translations: Map<number, StreamingTranslationEntry>,
  targetLanguage: StreamingTranslationTargetLanguage,
): string {
  const targetLabel = STREAMING_TARGET_LANGUAGE_NAMES[targetLanguage];
  const blocks = paragraphs.map(({ key, sourceText }) => {
    const entry = translations.get(key);
    const isCurrent = entry !== undefined && entry.sourceText === sourceText;
    let translatedText: string;
    if (isCurrent && entry.status === "mirrored") {
      // No model call was made for a mirrored paragraph — export the same
      // text the muted on-screen right cell shows (its own source text).
      translatedText = entry.translatedText ?? sourceText;
    } else if (
      isCurrent &&
      entry.status === "done" &&
      entry.translatedText !== undefined
    ) {
      translatedText = entry.translatedText;
    } else if (isCurrent && entry.status === "failed") {
      translatedText = STREAMING_TRANSLATION_FAILED_PLACEHOLDER;
    } else {
      translatedText = STREAMING_TRANSLATION_PLACEHOLDER;
    }
    return `Original:\n${sourceText}\n\n${targetLabel}:\n${translatedText}`;
  });
  return blocks.join("\n\n");
}
