/**
 * A meeting's status is shown in two places at once — the sidebar row's dot
 * (colour, tooltip, and accessible name) and the header status widget. They
 * previously each derived their own text and colour and drifted apart, so both
 * now resolve through this single function.
 *
 * The store only ever persists `no_files`, `ready`, and `finished` (see
 * `src-tauri/src/meetings.rs`). "Transcribing" and "error" are transient,
 * front-end-only states that outlive no reload, so they arrive as an explicit
 * activity override rather than as a stored value.
 */

export type MeetingStatusTone =
  "no-files" | "ready" | "transcribing" | "finished" | "error" | "unknown";

/** A transient, front-end-only state that overrides the persisted status. */
export type MeetingActivity = "none" | "transcribing" | "error";

export interface MeetingStatusView {
  /** Drives the `wp-tone--*` colour class on every surface. */
  tone: MeetingStatusTone;
  /** The user-visible label; never the raw store value. */
  label: string;
}

// A Map, not an object literal, so a meeting whose status happens to collide
// with an Object.prototype key ("constructor", "toString") still falls through
// to the unknown case instead of resolving to a prototype member.
const PERSISTED = new Map<string, MeetingStatusView>([
  ["no_files", { tone: "no-files", label: "No files" }],
  ["ready", { tone: "ready", label: "Ready" }],
  ["finished", { tone: "finished", label: "Finished" }],
]);

const ACTIVITY = new Map<MeetingActivity, MeetingStatusView>([
  ["transcribing", { tone: "transcribing", label: "Transcribing" }],
  ["error", { tone: "error", label: "Error" }],
]);

const UNKNOWN: MeetingStatusView = { tone: "unknown", label: "Unknown" };

export function resolveMeetingStatus(
  persisted: string | undefined,
  activity: MeetingActivity = "none",
): MeetingStatusView {
  // A run in flight — or the failure that just ended one — describes the
  // meeting better than the status the store still holds from before it began.
  const override = ACTIVITY.get(activity);
  if (override) return override;
  return (persisted && PERSISTED.get(persisted)) || UNKNOWN;
}
