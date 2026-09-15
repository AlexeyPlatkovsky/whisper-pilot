import {
  useEffect,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import {
  getLiveCaptureSnapshot,
  onLiveCaptureState,
  openStreamingSession,
  type LiveCaptureSnapshot,
  type StreamingTranslationTargetLanguage,
  type StreamingWindow,
} from "./ipc";
import {
  isLiveCaptureActive,
  reconcileLiveCaptureSnapshot,
} from "./liveCaptureState";
import { upsertWindow } from "./streamingText";

interface StreamingCaptureLifecycleProps {
  activeIdRef: MutableRefObject<number | null>;
  liveSnapshotRef: MutableRefObject<LiveCaptureSnapshot | null>;
  openRequestRef: MutableRefObject<number>;
  captureElapsedBaselineRef: MutableRefObject<number>;
  startTimeRef: MutableRefObject<number | null>;
  setCaptureHydrated: Dispatch<SetStateAction<boolean>>;
  setLiveCapturePhase: Dispatch<SetStateAction<LiveCaptureSnapshot["phase"]>>;
  setIsRunning: Dispatch<SetStateAction<boolean>>;
  setLiveCaptureSessionId: Dispatch<SetStateAction<number | null>>;
  setActiveId: Dispatch<SetStateAction<number | null>>;
  setActiveTitle: Dispatch<SetStateAction<string>>;
  setActiveSessionEngine: Dispatch<SetStateAction<"local" | "cloud" | null>>;
  setTranscriptionEngine: Dispatch<SetStateAction<"local" | "cloud">>;
  setWindows: Dispatch<SetStateAction<StreamingWindow[]>>;
  setTranslationEnabled: Dispatch<SetStateAction<boolean>>;
  commitTargetLanguage: (language: StreamingTranslationTargetLanguage) => void;
  setElapsed: Dispatch<SetStateAction<number>>;
  setError: Dispatch<SetStateAction<string | null>>;
}

/** Rehydrates capture from Rust and keeps its renderer lifecycle subscription scoped to one mount. */
export function useStreamingCaptureLifecycle({
  activeIdRef,
  liveSnapshotRef,
  openRequestRef,
  captureElapsedBaselineRef,
  startTimeRef,
  setCaptureHydrated,
  setLiveCapturePhase,
  setIsRunning,
  setLiveCaptureSessionId,
  setActiveId,
  setActiveTitle,
  setActiveSessionEngine,
  setTranscriptionEngine,
  setWindows,
  setTranslationEnabled,
  commitTargetLanguage,
  setElapsed,
  setError,
}: StreamingCaptureLifecycleProps) {
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    const applySnapshot = (incoming: LiveCaptureSnapshot) => {
      if (cancelled) return null;
      const current = liveSnapshotRef.current;
      const next = reconcileLiveCaptureSnapshot(current, incoming);
      if (next === current) return current;
      liveSnapshotRef.current = next;
      setCaptureHydrated(true);
      setLiveCapturePhase(next.phase);
      const belongsToStreaming =
        next.source === "streaming" || next.source == null;
      const active = belongsToStreaming && isLiveCaptureActive(next);
      setIsRunning(active);
      setLiveCaptureSessionId(belongsToStreaming ? next.session_id : null);
      if (active && next.session_id !== null)
        setActiveId((currentId) => currentId ?? next.session_id);
      if (
        belongsToStreaming &&
        next.phase === "error" &&
        next.error &&
        next.session_id === activeIdRef.current
      )
        setError(next.error);
      return next;
    };
    const rehydrate = async (snapshot: LiveCaptureSnapshot) => {
      const belongsToStreaming =
        snapshot.source === "streaming" || snapshot.source === null;
      if (
        !belongsToStreaming ||
        !isLiveCaptureActive(snapshot) ||
        snapshot.session_id === null
      )
        return;
      const sessionId = snapshot.session_id;
      const request = ++openRequestRef.current;
      try {
        const session = await openStreamingSession(sessionId);
        const latest = liveSnapshotRef.current;
        const stillActive =
          latest !== null &&
          isLiveCaptureActive(latest) &&
          (latest.source === "streaming" || latest.source === null) &&
          latest.session_id === sessionId;
        if (cancelled || request !== openRequestRef.current || !stillActive)
          return;
        setActiveTitle(session.title);
        setActiveSessionEngine(session.transcription_engine ?? null);
        if (session.transcription_engine)
          setTranscriptionEngine(session.transcription_engine);
        setWindows((currentWindows) =>
          currentWindows.reduce(
            (merged, window) => upsertWindow(merged, window),
            session.windows,
          ),
        );
        setTranslationEnabled(session.translation_enabled ?? false);
        commitTargetLanguage(session.translation_target_language ?? "ru");
        const lastWindow = session.windows.at(-1);
        if (lastWindow) {
          const resumedSeconds = Math.floor(lastWindow.end_ms / 1000);
          captureElapsedBaselineRef.current = resumedSeconds;
          if (startTimeRef.current !== null) {
            startTimeRef.current = Date.now() - resumedSeconds * 1000;
            setElapsed(resumedSeconds);
          }
        }
      } catch (reason) {
        if (!cancelled && request === openRequestRef.current)
          setError(String(reason));
      }
    };
    void (async () => {
      try {
        const stopListening = await onLiveCaptureState(applySnapshot);
        if (cancelled) {
          stopListening();
          return;
        }
        unlisten = stopListening;
        const snapshot = applySnapshot(await getLiveCaptureSnapshot());
        if (snapshot) await rehydrate(snapshot);
      } catch {
        // Commands remain usable when snapshot hydration fails.
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // This bridge subscription belongs to the renderer mount. Re-subscribing
    // on a controller render races a fresh session's persisted metadata.
  }, []);
}
