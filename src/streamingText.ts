// Pure streaming transcript helpers: window text/export formatting and the
// live-window merge. Kept out of the component so they are unit-testable
// without rendering.

import type { StreamingWindow } from "./ipc";

/** `m:ss` from a millisecond timestamp. */
export function formatClockTime(ms: number): string {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** Merge a live window into the ordered list, replacing a prior result for
 * the same index rather than duplicating it — the decode loop can, in
 * principle, resend an index (e.g. a future retry), and window order in the
 * transcript must always follow `window_index`, not arrival order. */
export function upsertWindow(
  windows: StreamingWindow[],
  incoming: StreamingWindow,
): StreamingWindow[] {
  const next = windows.filter((w) => w.window_index !== incoming.window_index);
  next.push(incoming);
  next.sort((a, b) => a.window_index - b.window_index);
  return next;
}

export function sourcesLabel(sources: {
  mic: boolean;
  system_audio: boolean;
}): string {
  if (sources.mic && sources.system_audio) return "Mic + System audio";
  if (sources.mic) return "Mic only";
  if (sources.system_audio) return "System audio only";
  return "No audio source";
}

/** A window's text for display/export — the same `[unavailable]` marker the
 * live transcript shows for a fail-open window, so copy/export output
 * matches what's on screen rather than silently dropping or blanking a
 * failed span. */
export function windowText(w: StreamingWindow): string {
  return w.outcome_ok ? w.text : "[unavailable]";
}

export function plainTranscript(windows: StreamingWindow[]): string {
  return windows.map(windowText).join(" ").replace(/\s+/g, " ").trim();
}

export function toMarkdown(title: string, text: string): string {
  return `# ${title}\n\n${text}\n`;
}

export function fileNameFor(title: string): string {
  const slug = title.replace(/[^\w\- ]+/g, "").trim();
  return `${slug || "streaming-session"}.md`;
}

// --- WP-103: per-window Live Translation entries & display -----------------
// Translation moved from one entry per *paragraph* (WP-93/WP-94/WP-100) to
// one entry per *window*, keyed by that window's own `window_index`. These
// helpers are shared by StreamingView.tsx (the live component, which also
// needs a "translating" spinner and a "Pending…" label per window) and
// export.ts's renderStreamingPaired (which only needs plain text), so both
// stay in lock-step with what the on-screen cell actually shows.

export type TranslationStatus =
  "pending" | "translating" | "done" | "mirrored" | "failed";

/** One window's Live Translation state (WP-103), keyed by its own
 * `window_index`. `sourceText` is that window's own text this entry was
 * produced from — comparing it against the window's *current* text
 * (`windowText`, not a joined multi-window string) is how a stale entry
 * (the window's text changed since, e.g. a fail-open retry) is detected and
 * replaced. */
export interface TranslationEntry {
  status: TranslationStatus;
  sourceText: string;
  translatedText?: string;
}

/** Placeholder for a window with no usable translation yet — missing, still
 * pending/translating, or one whose stored source text no longer matches the
 * window's current text (stale). Distinct from `TRANSLATION_FAILED_PLACEHOLDER`
 * below — see `windowTranslationDisplay`. */
export const TRANSLATION_PLACEHOLDER = "[Not translated]";

/** Placeholder for a window whose translation ran and errored. Distinct
 * wording from `TRANSLATION_PLACEHOLDER` because a failure needs a manual
 * retry rather than more waiting. */
export const TRANSLATION_FAILED_PLACEHOLDER = "[Translation failed]";

/** How one window's slice of a paragraph's translated cell should render:
 * `"text"` (real translated text, `mirrored` distinguishing the muted
 * same-language case from a genuine model translation) for `done`/
 * `mirrored`, or an inline placeholder kind for everything else — missing,
 * `pending`, `translating`, `failed`, or a stale source text. */
export type WindowTranslationDisplay =
  | { kind: "text"; text: string; mirrored: boolean }
  | { kind: "translating" }
  | { kind: "failed" }
  | { kind: "pending" };

/** Maps one window through its own translation entry (WP-103) — the single
 * place both the on-screen paired grid and `paragraphTranslatedText` below
 * decide what a window's translated slice looks like, so they can never
 * drift apart. A stale entry (its stored `sourceText` no longer matches the
 * window's current text) is treated the same as a missing one. */
export function windowTranslationDisplay(
  w: StreamingWindow,
  entry: TranslationEntry | undefined,
): WindowTranslationDisplay {
  const isCurrent = entry !== undefined && entry.sourceText === windowText(w);
  if (isCurrent) {
    if (entry.status === "mirrored") {
      return {
        kind: "text",
        text: entry.translatedText ?? windowText(w),
        mirrored: true,
      };
    }
    if (entry.status === "done" && entry.translatedText !== undefined) {
      return { kind: "text", text: entry.translatedText, mirrored: false };
    }
    if (entry.status === "failed") return { kind: "failed" };
    if (entry.status === "translating") return { kind: "translating" };
  }
  return { kind: "pending" };
}

/**
 * A paragraph's translated text (WP-103) — maps each window through
 * `windowTranslationDisplay` and joins the results the way `plainTranscript`
 * joins the original, so a still-translating trailing window renders as a
 * placeholder while finished windows show real text. Used by export.ts; the
 * on-screen renderer calls `windowTranslationDisplay` directly for its
 * spinner/"Pending…" states.
 */
export function paragraphTranslatedText(
  paragraph: StreamingWindow[],
  translations: Map<number, TranslationEntry>,
): string {
  return paragraph
    .map((w) => {
      const display = windowTranslationDisplay(
        w,
        translations.get(w.window_index),
      );
      if (display.kind === "text") return display.text;
      if (display.kind === "failed") return TRANSLATION_FAILED_PLACEHOLDER;
      return TRANSLATION_PLACEHOLDER;
    })
    .join(" ")
    .replace(/\s+/g, " ")
    .trim();
}
