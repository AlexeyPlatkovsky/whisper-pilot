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
import { stabilizePartial, type StablePartial } from "./partialStability";

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
  const [partialDisplay, setPartialDisplay] = useState<StablePartial>({
    stable: "",
    unstable: "",
  });
  const [shortcut, setShortcut] = useState("Control+Option+Space");
  const sessionId = useRef<number | null>(null);
  const previousPartial = useRef("");

  const clearPartial = () => {
    previousPartial.current = "";
    setPartial("");
    setPartialDisplay({ stable: "", unstable: "" });
  };

  const showTransientMessage = (message: string) => {
    previousPartial.current = "";
    setPartial(message);
    setPartialDisplay({ stable: "", unstable: message });
  };

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
            showTransientMessage(String(error));
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
            clearPartial();
            void refreshShortcut();
          }
          if (sessionId.current === session.id)
            setStatus(STATUS[session.status]);
        }),
        await onRecorderSegmentCommitted((segment) => {
          if (cancelled || segment.session_id !== sessionId.current) return;
          setCommitted(segment.text);
          clearPartial();
        }),
        await onRecorderPartial((next) => {
          if (!cancelled && next.session_id === sessionId.current) {
            setPartialDisplay(
              stabilizePartial(previousPartial.current, next.text),
            );
            previousPartial.current = next.text;
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
          showTransientMessage(error.message);
        }),
      );
      const [snapshot, shortcutStatus] = await Promise.all([
        getLiveCaptureSnapshot(),
        refreshShortcut(),
      ]);
      if (cancelled) return;
      if (shortcutStatus?.error) {
        setStatus("Shortcut disabled");
        showTransientMessage(shortcutStatus.error);
      }
      if (snapshot.source === "recorder" && snapshot.session_id !== null) {
        const session = await openRecorderSession(snapshot.session_id);
        if (cancelled) return;
        sessionId.current = session.id;
        setStatus(STATUS[session.status]);
        setCommitted(session.segments.at(-1)?.text ?? "");
        clearPartial();
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
          showTransientMessage(String(error));
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
        {partialDisplay.stable && <span>{partialDisplay.stable} </span>}
        {partialDisplay.unstable && <em>{partialDisplay.unstable}</em>}
        {!committed && !partial && <em>Start speaking…</em>}
      </p>
    </main>
  );
}
