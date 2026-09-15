import { useMemo } from "react";
import type { LiveCaptureSnapshot, StreamingSessionSummary } from "./ipc";
import { resolveStreamingWidgetStatus } from "./streamingStatus";

export function useStreamingViewDerived({
  sessions,
  sessionSearch,
  activeId,
  liveCaptureSessionId,
  isRunning,
  liveCapturePhase,
  busy,
  craftingId,
  prettifyingId,
  craftFailedId,
  prettifyFailedId,
}: {
  sessions: StreamingSessionSummary[];
  sessionSearch: string;
  activeId: number | null;
  liveCaptureSessionId: number | null;
  isRunning: boolean;
  liveCapturePhase: LiveCaptureSnapshot["phase"];
  busy: boolean;
  craftingId: number | null;
  prettifyingId: number | null;
  craftFailedId: number | null;
  prettifyFailedId: number | null;
}) {
  const selectedOwnsLiveCapture =
    isRunning && activeId !== null && activeId === liveCaptureSessionId;
  const isCraftingActive = craftingId !== null && craftingId === activeId;
  const isPrettifyingActive =
    prettifyingId !== null && prettifyingId === activeId;
  const craftFailed = craftFailedId !== null && craftFailedId === activeId;
  const prettifyFailed =
    prettifyFailedId !== null && prettifyFailedId === activeId;
  const widget = resolveStreamingWidgetStatus(
    selectedOwnsLiveCapture && liveCapturePhase === "starting"
      ? "starting"
      : selectedOwnsLiveCapture
        ? "on-air"
        : busy
          ? "starting"
          : isCraftingActive
            ? "crafting"
            : isPrettifyingActive
              ? "prettifying"
              : craftFailed
                ? "mfu-failed"
                : prettifyFailed
                  ? "prettify-failed"
                  : "ready",
  );
  const liveCaptureWidget = resolveStreamingWidgetStatus(
    liveCapturePhase === "starting" ? "starting" : "on-air",
  );
  const filteredSessions = useMemo(() => {
    const query = sessionSearch.trim().toLocaleLowerCase();
    return query.length < 3
      ? sessions
      : sessions.filter((session) =>
          session.title.toLocaleLowerCase().includes(query),
        );
  }, [sessionSearch, sessions]);
  return {
    selectedOwnsLiveCapture,
    isCraftingActive,
    isPrettifyingActive,
    craftFailed,
    prettifyFailed,
    widget,
    liveCaptureWidget,
    filteredSessions,
  };
}
