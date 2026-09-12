import { useEffect, useRef, useState } from "react";
import {
  getLiveCaptureSnapshot,
  getRecorderShortcutStatus,
  onRecorderError,
  onRecorderPartial,
  onRecorderSegmentCommitted,
  onRecorderSessionChanged,
  openRecorderSession,
  showRecorderWorkspace,
  type RecorderStatus,
} from "./ipc";
import { AppLogo } from "./Icon";

const STATUS: Record<RecorderStatus, string> = {
  recording: "Listening",
  finalizing: "Finalizing",
  completed: "Completed",
  recoverable: "Recovery required",
  delete_failed: "Cleanup failed",
};

export function RecorderCaption() {
  const [status, setStatus] = useState("Listening");
  const [committed, setCommitted] = useState("");
  const [partial, setPartial] = useState("");
  const [shortcut, setShortcut] = useState("Control+Option+Space");
  const sessionId = useRef<number | null>(null);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    void (async () => {
      const refreshShortcut = async () => {
        try {
          const current = await getRecorderShortcutStatus();
          if (!cancelled) setShortcut(current.configured);
          return current;
        } catch (error) {
          if (!cancelled) {
            setStatus("Shortcut status unavailable");
            setPartial(String(error));
          }
          return null;
        }
      };
      unlisteners.push(
        await onRecorderSessionChanged((session) => {
          if (cancelled) return;
          if (
            session.status === "recording" &&
            sessionId.current !== session.id
          ) {
            sessionId.current = session.id;
            setCommitted("");
            setPartial("");
            void refreshShortcut();
          }
          if (sessionId.current === session.id)
            setStatus(STATUS[session.status]);
        }),
        await onRecorderSegmentCommitted((segment) => {
          if (cancelled || segment.session_id !== sessionId.current) return;
          setCommitted(segment.text);
          setPartial("");
        }),
        await onRecorderPartial((next) => {
          if (!cancelled && next.session_id === sessionId.current) {
            setPartial(next.text);
          }
        }),
        await onRecorderError((error) => {
          if (
            cancelled ||
            (error.session_id !== undefined &&
              error.session_id !== sessionId.current)
          ) {
            return;
          }
          setStatus("Recorder error");
          setPartial(error.message);
        }),
      );
      const [snapshot, shortcutStatus] = await Promise.all([
        getLiveCaptureSnapshot(),
        refreshShortcut(),
      ]);
      if (cancelled) return;
      if (shortcutStatus?.error) {
        setStatus("Shortcut disabled");
        setPartial(shortcutStatus.error);
      }
      if (snapshot.source === "recorder" && snapshot.session_id !== null) {
        const session = await openRecorderSession(snapshot.session_id);
        if (cancelled) return;
        sessionId.current = session.id;
        setStatus(STATUS[session.status]);
        setCommitted(session.segments.at(-1)?.text ?? "");
        setPartial("");
      }
    })();
    return () => {
      cancelled = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  return (
    <main
      className="recorder-caption"
      aria-label="Open Recorder workspace"
      aria-live="polite"
      role="button"
      tabIndex={0}
      onClick={() =>
        void showRecorderWorkspace().catch((error) => {
          setStatus("Recorder error");
          setPartial(String(error));
        })
      }
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          event.currentTarget.click();
        }
      }}
    >
      <div className="recorder-caption__status">
        <AppLogo size={24} />
        <strong>{status}</strong>
        <span>{shortcut.replaceAll("+", " ")}</span>
      </div>
      <p>
        {committed && <span>{committed} </span>}
        {partial && <em>{partial}</em>}
        {!committed && !partial && <em>Start speaking…</em>}
      </p>
    </main>
  );
}
