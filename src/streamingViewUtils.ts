import type { StreamingWindow } from "./ipc";
import { formatClockTime } from "./streamingText";

export function streamingDurationLabel(windows: StreamingWindow[]) {
  return windows.length === 0 ? "—" : formatClockTime(windows.at(-1)!.end_ms);
}
