import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import {
  getLiveCaptureSnapshot,
  onLiveCaptureState,
  restoreMainFromBubble,
  type LiveCaptureSnapshot,
} from "./ipc";
import { AppLogo, Icon } from "./Icon";

type BubbleSnapshot = Pick<LiveCaptureSnapshot, "phase" | "source">;

export interface BubbleWindowAdapter {
  startDragging(): Promise<void>;
  restoreMainWindow(): Promise<void>;
}

const nativeWindowAdapter: BubbleWindowAdapter = {
  startDragging: () => getCurrentWindow().startDragging(),
  restoreMainWindow: restoreMainFromBubble,
};

export function bubblePresentation(
  snapshot: BubbleSnapshot,
  error: string | null,
) {
  if (error) {
    return {
      kind: "error" as const,
      label: `WhisperPilot error: ${error}`,
      symbol: "alert" as const,
    };
  }
  if (
    snapshot.source !== null &&
    ["starting", "capturing", "stopping"].includes(snapshot.phase)
  ) {
    return {
      kind: "listening" as const,
      label:
        snapshot.source === "recorder"
          ? "WhisperPilot listening to microphone"
          : "WhisperPilot listening to system audio",
      symbol: "mic" as const,
    };
  }
  return {
    kind: "idle" as const,
    label: "WhisperPilot idle",
    symbol: "pause" as const,
  };
}

export function RecorderBubble({
  windowAdapter = nativeWindowAdapter,
}: {
  windowAdapter?: BubbleWindowAdapter;
}) {
  const [snapshot, setSnapshot] = useState<BubbleSnapshot>({
    phase: "idle",
    source: null,
  });
  const [error, setError] = useState<string | null>(null);
  const gesture = useRef<{ x: number; y: number; dragging: boolean } | null>(
    null,
  );
  const suppressNextClick = useRef(false);

  useEffect(() => {
    document.documentElement.classList.add("bubble-document");
    document.body.classList.add("bubble-document");
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    if (windowAdapter !== nativeWindowAdapter) {
      return () => {
        document.documentElement.classList.remove("bubble-document");
        document.body.classList.remove("bubble-document");
      };
    }
    void (async () => {
      try {
        unlisten = await onLiveCaptureState((next) => {
          if (cancelled) return;
          setSnapshot(next);
          setError(next.error ?? null);
        });
        const current = await getLiveCaptureSnapshot();
        if (!cancelled) {
          setSnapshot(current);
          setError(current.error ?? null);
        }
      } catch (reason) {
        if (!cancelled) setError(String(reason));
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
      document.documentElement.classList.remove("bubble-document");
      document.body.classList.remove("bubble-document");
    };
  }, [windowAdapter]);

  const presentation = bubblePresentation(snapshot, error);

  async function restore() {
    try {
      await windowAdapter.restoreMainWindow();
    } catch (reason) {
      setError(String(reason));
    }
  }

  return (
    <main className="recorder-bubble-shell">
      <button
        type="button"
        className={`recorder-bubble recorder-bubble--${presentation.kind}`}
        aria-label={presentation.label}
        title={`${presentation.label}. Drag to move; click to restore.`}
        onPointerDown={(event) => {
          // A genuine new pointer gesture supersedes any synthetic click that
          // the previous native drag might have omitted.
          suppressNextClick.current = false;
          gesture.current = {
            x: event.clientX,
            y: event.clientY,
            dragging: false,
          };
          event.currentTarget.setPointerCapture?.(event.pointerId);
        }}
        onPointerMove={(event) => {
          const current = gesture.current;
          if (!current || current.dragging) return;
          if (
            Math.hypot(event.clientX - current.x, event.clientY - current.y) > 5
          ) {
            current.dragging = true;
            suppressNextClick.current = true;
            void windowAdapter.startDragging().catch((reason) => {
              setError(String(reason));
            });
          }
        }}
        onPointerUp={() => {
          gesture.current = null;
        }}
        onPointerCancel={() => {
          gesture.current = null;
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            suppressNextClick.current = false;
          }
        }}
        onClick={() => {
          if (suppressNextClick.current) {
            suppressNextClick.current = false;
            return;
          }
          void restore();
        }}
      >
        <span className="recorder-bubble__ring" aria-hidden="true" />
        <AppLogo size={58} />
        <span className="recorder-bubble__state" aria-hidden="true">
          {presentation.symbol === "alert" ? (
            <Icon name="alert-circle" size={20} />
          ) : presentation.symbol === "mic" ? (
            <Icon name="mic" size={20} />
          ) : (
            <Icon name="pause" size={20} />
          )}
        </span>
      </button>
    </main>
  );
}
