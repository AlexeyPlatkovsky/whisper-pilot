// Single source of truth for both the sidebar dot and the header status
// widget, so the two can never drift apart. "Transcribing"/"diarizing"/"error"
// are front-end-only activity overrides, not stored values — see
// src-tauri/src/meetings/ for the persisted status set.

import type { IconName } from "./Icon";
import type { StatusColorKey } from "./statusColors";

export type MeetingStatusTone =
  | "no-files"
  | "ready"
  | "transcribing"
  | "diarizing"
  | "crafting"
  | "finished"
  | "error"
  | "no-model"
  | "unknown";

/** A transient, front-end-only state that overrides the persisted status. */
export type MeetingActivity =
  "none" | "transcribing" | "diarizing" | "crafting" | "error" | "no-model";

export interface MeetingStatusView {
  /** Drives the `wp-tone--*` colour class on every surface. */
  tone: MeetingStatusTone;
  /** The user-visible label; never the raw store value. */
  label: string;
  /** The same visual status cue used by Streaming's header widget. */
  icon: IconName;
  /** Which configurable status color (WP-88) this view resolves to — drives
   * the `wp-status--*` class that overrides the tone's default color. */
  statusKey: StatusColorKey;
}

// A Map, not an object literal, so a meeting whose status happens to collide
// with an Object.prototype key ("constructor", "toString") still falls through
// to the unknown case instead of resolving to a prototype member.
const PERSISTED = new Map<string, MeetingStatusView>([
  [
    "no_files",
    {
      tone: "no-files",
      label: "No files",
      icon: "info",
      statusKey: "no-files",
    },
  ],
  [
    "ready",
    { tone: "ready", label: "Ready", icon: "check", statusKey: "ready" },
  ],
  [
    "finished",
    {
      tone: "finished",
      label: "Finished",
      icon: "check",
      statusKey: "finished",
    },
  ],
]);

const ACTIVITY = new Map<MeetingActivity, MeetingStatusView>([
  [
    "transcribing",
    {
      tone: "transcribing",
      label: "Transcribing",
      icon: "refresh-cw",
      statusKey: "transcribing",
    },
  ],
  [
    "diarizing",
    {
      tone: "diarizing",
      label: "Diarizing",
      icon: "refresh-cw",
      statusKey: "diarizing",
    },
  ],
  [
    "crafting",
    {
      tone: "crafting",
      label: "Crafting notes",
      icon: "refresh-cw",
      statusKey: "crafting-notes",
    },
  ],
  [
    "error",
    {
      tone: "error",
      label: "Error",
      icon: "alert-circle",
      statusKey: "error",
    },
  ],
  [
    "no-model",
    {
      tone: "no-model",
      label: "No model",
      icon: "alert-circle",
      statusKey: "no-model",
    },
  ],
]);

const UNKNOWN: MeetingStatusView = {
  tone: "unknown",
  label: "Unknown",
  icon: "alert-circle",
  statusKey: "unknown",
};

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
