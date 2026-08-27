import { describe, expect, it } from "vitest";
import {
  exportFileExtension,
  renderForExport,
  renderMarkdown,
  renderPlainText,
} from "./export";
import type { MeetingMfu, Segment } from "./ipc";
// WP-94: Streaming Copy/Export paired original+translation renderer — kept
// as a separate import so the block above (Meeting's existing renderer) is
// untouched.
import {
  hasStreamingTranslations,
  renderStreamingPaired,
  STREAMING_TRANSLATION_FAILED_PLACEHOLDER,
  STREAMING_TRANSLATION_PLACEHOLDER,
  type StreamingTranslationEntry,
} from "./export";

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

// WP-94: Streaming Copy/Export paired original+translation rendering — a
// pure renderer decoupled from StreamingView's component state so it's
// unit-testable without rendering. Wiring coverage (Copy button, Export,
// same-render-path guarantee) lives in StreamingView.pairedExport.test.tsx.

function entry(
  status: StreamingTranslationEntry["status"],
  sourceText: string,
  translatedText?: string,
): StreamingTranslationEntry {
  return { status, sourceText, translatedText };
}

describe("renderStreamingPaired", () => {
  it("@WP-94-happy-paired-export: pairs each paragraph's original with its translation, in screen order, labelled by source side and target language", () => {
    const paragraphs = [
      { key: 0, sourceText: "Привет всем." },
      { key: 6, sourceText: "Вторая часть." },
    ];
    const translations = new Map<number, StreamingTranslationEntry>([
      [0, entry("done", "Привет всем.", "Hello everyone.")],
      [6, entry("done", "Вторая часть.", "Second part.")],
    ]);

    const text = renderStreamingPaired(paragraphs, translations, "en");

    expect(text).toBe(
      [
        "Original:",
        "Привет всем.",
        "",
        "English:",
        "Hello everyone.",
        "",
        "Original:",
        "Вторая часть.",
        "",
        "English:",
        "Second part.",
      ].join("\n"),
    );
  });

  it("labels the translated side with the target language's display name (Russian)", () => {
    const paragraphs = [{ key: 0, sourceText: "Hello." }];
    const translations = new Map<number, StreamingTranslationEntry>([
      [0, entry("done", "Hello.", "Привет.")],
    ]);

    const text = renderStreamingPaired(paragraphs, translations, "ru");

    expect(text).toContain("Русский:");
    expect(text).toContain("Привет.");
  });

  // EP: every not-yet-translated class (never attempted, in flight, or
  // stale) collapses to the same "not translated" placeholder rather than
  // being omitted or left blank. A *failed* attempt is deliberately excluded
  // from this group — see the dedicated failed-placeholder test below — and
  // a *mirrored* paragraph is excluded too, since it isn't a placeholder
  // case at all — see @WP-94-mirrored.
  it.each([
    ["missing entry entirely", undefined],
    ["pending", entry("pending", "Same text.")],
    ["translating", entry("translating", "Same text.")],
  ] as const)(
    "@WP-94-placeholder: emits the not-translated placeholder for %s",
    (_label, maybeEntry) => {
      const paragraphs = [{ key: 0, sourceText: "Same text." }];
      const translations = new Map<number, StreamingTranslationEntry>();
      if (maybeEntry) translations.set(0, maybeEntry);

      const text = renderStreamingPaired(paragraphs, translations, "en");

      expect(text).toBe(
        [
          "Original:",
          "Same text.",
          "",
          "English:",
          STREAMING_TRANSLATION_PLACEHOLDER,
        ].join("\n"),
      );
    },
  );

  // A failed translation was attempted and errored — categorically different
  // from "hasn't happened yet". Reusing the not-yet-translated wording would
  // read to a user as "still waiting", inviting them to expect it to resolve
  // on its own, when in fact (per StreamingView's reconciliation) retry is
  // manual only. A distinct wording is the least-misleading choice.
  it("@WP-94-placeholder-failed: emits a distinct failed-translation placeholder, not the not-yet-translated wording", () => {
    const paragraphs = [{ key: 0, sourceText: "Same text." }];
    const translations = new Map<number, StreamingTranslationEntry>([
      [0, entry("failed", "Same text.")],
    ]);

    const text = renderStreamingPaired(paragraphs, translations, "en");

    expect(text).toBe(
      [
        "Original:",
        "Same text.",
        "",
        "English:",
        STREAMING_TRANSLATION_FAILED_PLACEHOLDER,
      ].join("\n"),
    );
    expect(STREAMING_TRANSLATION_FAILED_PLACEHOLDER).not.toBe(
      STREAMING_TRANSLATION_PLACEHOLDER,
    );
  });

  // A mirrored paragraph is not a failure at all: its windows are already
  // entirely in the target language, so no model call was made by design and
  // the on-screen right cell shows the paragraph's own text in a muted
  // style. Copy/Export reproduce what the screen shows, so the exported
  // right cell must carry that same text verbatim rather than a placeholder.
  it("@WP-94-mirrored: exports a mirrored paragraph's own text verbatim — the same text the on-screen muted cell shows — not a placeholder", () => {
    const paragraphs = [{ key: 0, sourceText: "Same text." }];
    const translations = new Map<number, StreamingTranslationEntry>([
      [0, entry("mirrored", "Same text.", "Same text.")],
    ]);

    const text = renderStreamingPaired(paragraphs, translations, "en");

    expect(text).toBe(
      ["Original:", "Same text.", "", "English:", "Same text."].join("\n"),
    );
  });

  it("@WP-94-mirrored-fallback: falls back to the paragraph's current source text for a mirrored entry with no stored translatedText", () => {
    const paragraphs = [{ key: 0, sourceText: "Same text." }];
    const translations = new Map<number, StreamingTranslationEntry>([
      [0, { status: "mirrored", sourceText: "Same text." }],
    ]);

    const text = renderStreamingPaired(paragraphs, translations, "en");

    expect(text).toBe(
      ["Original:", "Same text.", "", "English:", "Same text."].join("\n"),
    );
  });

  it("@WP-94-mirrored-stale: a mirrored entry whose stored source text no longer matches the paragraph's current text falls back to the not-translated placeholder", () => {
    const paragraphs = [{ key: 0, sourceText: "Current text." }];
    const translations = new Map<number, StreamingTranslationEntry>([
      [0, entry("mirrored", "Old text.", "Old text.")],
    ]);

    const text = renderStreamingPaired(paragraphs, translations, "en");

    expect(text).toBe(
      [
        "Original:",
        "Current text.",
        "",
        "English:",
        STREAMING_TRANSLATION_PLACEHOLDER,
      ].join("\n"),
    );
  });

  it("@WP-94-stale: emits the not-translated placeholder when the stored entry's source text no longer matches the paragraph's current text", () => {
    const paragraphs = [{ key: 0, sourceText: "Current text." }];
    const translations = new Map<number, StreamingTranslationEntry>([
      [0, entry("done", "Old text.", "Translated old text.")],
    ]);

    const text = renderStreamingPaired(paragraphs, translations, "en");

    expect(text).toContain(STREAMING_TRANSLATION_PLACEHOLDER);
    expect(text).not.toContain("Translated old text.");
  });

  it("@WP-94-partial: leaves other paragraphs' translations intact when only one paragraph lacks a translation", () => {
    const paragraphs = [
      { key: 0, sourceText: "First." },
      { key: 6, sourceText: "Second." },
      { key: 12, sourceText: "Third." },
    ];
    const translations = new Map<number, StreamingTranslationEntry>([
      [0, entry("done", "First.", "First (en).")],
      [12, entry("done", "Third.", "Third (en).")],
      // paragraph at key 6 has no entry at all.
    ]);

    const text = renderStreamingPaired(paragraphs, translations, "en");

    expect(text).toContain("First (en).");
    expect(text).toContain("Third (en).");
    expect(text).toContain(STREAMING_TRANSLATION_PLACEHOLDER);
  });

  // Property: paragraph-count parity — regardless of the translation-status
  // mix, the paired output always has exactly one Original block and one
  // target-language block per input paragraph, so the two sides can never
  // drift out of count.
  it("@WP-94-parity: always emits one Original block and one target-language block per paragraph, whatever the status mix", () => {
    const statuses: (StreamingTranslationEntry["status"] | undefined)[] = [
      "done",
      "pending",
      "translating",
      "failed",
      "mirrored",
      undefined,
    ];
    const paragraphs = statuses.map((_, i) => ({
      key: i,
      sourceText: `Paragraph ${i}.`,
    }));
    const translations = new Map<number, StreamingTranslationEntry>();
    statuses.forEach((status, i) => {
      if (status) {
        translations.set(
          i,
          entry(status, `Paragraph ${i}.`, `Translated ${i}.`),
        );
      }
    });

    const text = renderStreamingPaired(paragraphs, translations, "en");

    expect(text.match(/^Original:$/gm)?.length).toBe(paragraphs.length);
    expect(text.match(/^English:$/gm)?.length).toBe(paragraphs.length);
  });

  it("renders an empty string for an empty paragraph list", () => {
    expect(renderStreamingPaired([], new Map(), "en")).toBe("");
  });
});

describe("hasStreamingTranslations", () => {
  it("@WP-94-off-byte-identical: is false for an empty translations map (Live Translation off, or on with nothing recorded yet)", () => {
    expect(hasStreamingTranslations(new Map())).toBe(false);
  });

  it("is true once at least one paragraph has a translation entry", () => {
    const translations = new Map<number, StreamingTranslationEntry>([
      [0, entry("pending", "text")],
    ]);
    expect(hasStreamingTranslations(translations)).toBe(true);
  });
});
