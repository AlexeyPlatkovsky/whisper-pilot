/**
 * Streaming's header status widget cycles through seven states (WP-76/77/75),
 * previously rendered as seven near-duplicate JSX blocks in StreamingView.
 * This mirrors `meetingStatus.ts`'s single-resolver approach: one table drives
 * both the header widget and the sidebar session-row dot, so the two can't
 * describe the same session differently.
 */

import type { IconName } from "./Icon";

export type StreamingStatusTone = "finished" | "unknown" | "error" | "crafting";

export interface StreamingStatusView {
  tone: StreamingStatusTone;
  label: string;
  icon: IconName;
  spinning: boolean;
  /** Whether the elapsed-time readout is shown alongside the label. */
  showTimer: boolean;
}

export type StreamingWidgetState =
  | "ready"
  | "starting"
  | "on-air"
  | "crafting"
  | "mfu-failed"
  | "prettifying"
  | "prettify-failed";

const WIDGET_STATES = new Map<StreamingWidgetState, StreamingStatusView>([
  [
    "ready",
    {
      tone: "finished",
      label: "Ready",
      icon: "check",
      spinning: false,
      showTimer: false,
    },
  ],
  [
    "starting",
    {
      tone: "unknown",
      label: "Starting…",
      icon: "refresh-cw",
      spinning: true,
      showTimer: false,
    },
  ],
  [
    "on-air",
    {
      tone: "error",
      label: "On Air",
      icon: "refresh-cw",
      spinning: true,
      showTimer: true,
    },
  ],
  [
    "crafting",
    {
      tone: "crafting",
      label: "Crafting MFU…",
      icon: "refresh-cw",
      spinning: true,
      showTimer: true,
    },
  ],
  [
    "mfu-failed",
    {
      tone: "error",
      label: "MFU Failed",
      icon: "alert-circle",
      spinning: false,
      showTimer: false,
    },
  ],
  [
    "prettifying",
    {
      tone: "crafting",
      label: "Prettifying…",
      icon: "refresh-cw",
      spinning: true,
      showTimer: true,
    },
  ],
  [
    "prettify-failed",
    {
      tone: "error",
      label: "Prettify Failed",
      icon: "alert-circle",
      spinning: false,
      showTimer: false,
    },
  ],
]);

export function resolveStreamingWidgetStatus(
  state: StreamingWidgetState,
): StreamingStatusView {
  // The map is seeded with every StreamingWidgetState variant above, so this
  // lookup can never miss.
  return WIDGET_STATES.get(state)!;
}

/** Sidebar row tone for a session that is not the one currently open and
 * running — only its persisted store status ("active" / "stopped", see
 * `src-tauri/src/streaming_store.rs::status`) is known for it. */
export function resolveStreamingRowStatus(
  persisted: string,
): StreamingStatusView {
  if (persisted === "stopped") {
    return {
      tone: "finished",
      label: "Finished",
      icon: "check",
      spinning: false,
      showTimer: false,
    };
  }
  // A session left "active" that is not the one currently running in this
  // window is a crash/quit-recovery edge case, not a normal state.
  return {
    tone: "unknown",
    label: "Unknown",
    icon: "alert-circle",
    spinning: false,
    showTimer: false,
  };
}
