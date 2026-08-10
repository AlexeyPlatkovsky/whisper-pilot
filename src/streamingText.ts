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
