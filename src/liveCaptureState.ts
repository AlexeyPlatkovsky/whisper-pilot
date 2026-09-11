export type LiveCapturePhase =
  "idle" | "starting" | "capturing" | "stopping" | "error";

export interface LiveCaptureSnapshot {
  phase: LiveCapturePhase;
  session_id: number | null;
  source?: "streaming" | "recorder" | null;
  generation: number;
  revision: number;
  error: string | null;
}

/** Resolve the subscribe-before-snapshot race by accepting revisions only. */
export function reconcileLiveCaptureSnapshot(
  current: LiveCaptureSnapshot | null,
  incoming: LiveCaptureSnapshot,
): LiveCaptureSnapshot {
  return current === null || incoming.revision > current.revision
    ? incoming
    : current;
}

export function isLiveCaptureActive(snapshot: LiveCaptureSnapshot | null) {
  return (
    snapshot?.phase === "starting" ||
    snapshot?.phase === "capturing" ||
    snapshot?.phase === "stopping"
  );
}
