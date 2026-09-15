import type {
  LiveCaptureSnapshot,
  RecorderSession,
  RecorderSessionSummary,
  RecorderStatus,
} from "./ipc";
import type { StreamingStatusView } from "./streamingStatus";

export function filterRecorderSessions(
  sessions: RecorderSessionSummary[],
  search: string,
) {
  const query = search.trim().toLocaleLowerCase();
  return query
    ? sessions.filter((session) =>
        session.title.toLocaleLowerCase().includes(query),
      )
    : sessions;
}

export function resolveRecorderError(
  error: string | null,
  snapshot: LiveCaptureSnapshot | null,
  active: RecorderSession | null,
) {
  const captureError =
    snapshot?.source === "recorder" &&
    snapshot.phase === "error" &&
    snapshot.session_id === active?.id
      ? snapshot.error
      : null;
  return error ?? captureError ?? active?.recovery_reason ?? null;
}

export function persistedRecorderStatus(
  status: RecorderStatus,
  isDraft = false,
): StreamingStatusView {
  if (isDraft) {
    return {
      tone: "finished",
      label: "Ready",
      icon: "check",
      spinning: false,
      showTimer: false,
      statusKey: "ready",
    };
  }
  switch (status) {
    case "recording":
      return {
        tone: "error",
        label: "Recording",
        icon: "refresh-cw",
        spinning: true,
        showTimer: true,
        statusKey: "on-air",
      };
    case "finalizing":
      return {
        tone: "unknown",
        label: "Finalizing",
        icon: "refresh-cw",
        spinning: true,
        showTimer: false,
        statusKey: "starting",
      };
    case "recoverable":
      return {
        tone: "error",
        label: "Recoverable",
        icon: "alert-circle",
        spinning: false,
        showTimer: false,
        statusKey: "error",
      };
    case "delete_failed":
      return {
        tone: "error",
        label: "Delete failed",
        icon: "alert-circle",
        spinning: false,
        showTimer: false,
        statusKey: "error",
      };
    case "completed":
      return {
        tone: "finished",
        label: "Completed",
        icon: "check",
        spinning: false,
        showTimer: false,
        statusKey: "finished",
      };
  }
}

export function recorderCaptureStatus(
  snapshot: LiveCaptureSnapshot | null,
): StreamingStatusView | null {
  if (snapshot?.source !== "recorder") return null;
  switch (snapshot.phase) {
    case "starting":
      return {
        tone: "unknown",
        label: "Starting",
        icon: "refresh-cw",
        spinning: true,
        showTimer: false,
        statusKey: "starting",
      };
    case "capturing":
      return persistedRecorderStatus("recording");
    case "stopping":
      return {
        tone: "unknown",
        label: "Stopping · Finalizing",
        icon: "refresh-cw",
        spinning: true,
        showTimer: false,
        statusKey: "starting",
      };
    case "error":
      return {
        tone: "error",
        label: "Error",
        icon: "alert-circle",
        spinning: false,
        showTimer: false,
        statusKey: "error",
      };
    case "idle":
      return null;
  }
}

export function resolveRecorderStatus(
  active: RecorderSession | null,
  snapshot: LiveCaptureSnapshot | null,
  polishing: boolean,
): StreamingStatusView {
  if (polishing) {
    return {
      tone: "crafting",
      label: "Prettifying…",
      icon: "refresh-cw",
      spinning: true,
      showTimer: false,
      statusKey: "prettifying",
    };
  }
  if (active?.is_draft) return persistedRecorderStatus(active.status, true);
  if (active !== null && snapshot?.session_id === active.id) {
    const captureStatus = recorderCaptureStatus(snapshot);
    if (captureStatus) return captureStatus;
  }
  return active
    ? persistedRecorderStatus(active.status)
    : {
        tone: "finished",
        label: snapshot ? "Ready" : "Loading",
        icon: snapshot ? "check" : "refresh-cw",
        spinning: snapshot === null,
        showTimer: false,
        statusKey: snapshot ? "ready" : "unknown",
      };
}
