import { useCallback, useEffect, useRef, useState } from "react";
import { listStreamingSessions, type StreamingSessionSummary } from "./ipc";

/** Revision-gated library hydration prevents an old list response replacing a newer user action. */
export function useStreamingSessionLibrary() {
  const [sessions, setSessions] = useState<StreamingSessionSummary[]>([]);
  const [sessionsHydrated, setSessionsHydrated] = useState(false);
  const requestRef = useRef(0);
  const refreshSessions = useCallback(async () => {
    const request = ++requestRef.current;
    try {
      const next = await listStreamingSessions();
      if (request === requestRef.current) setSessions(next);
    } finally {
      if (request === requestRef.current) setSessionsHydrated(true);
    }
  }, []);
  useEffect(() => {
    // Initial hydration is intentionally non-blocking. Its failure must not
    // escape React as an unhandled promise rejection; callers may retry via a
    // later lifecycle event or user action.
    void refreshSessions().catch(() => undefined);
  }, [refreshSessions]);
  return { sessions, sessionsHydrated, refreshSessions };
}
