import { useCallback, useEffect, useState } from "react";
import {
  deleteStreamingSession,
  listStreamingSessions,
  onStreamingSessionEnded,
  onStreamingSources,
  onStreamingWindow,
  openStreamingSession,
  renameStreamingSession,
  saveTextDialog,
  startStreamingSession,
  stopStreamingSession,
  type StreamingSessionSummary,
  type StreamingWindow,
} from "./ipc";
import { AppLogo, Icon } from "./Icon";

function formatClockTime(ms: number): string {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** Merge a live window into the ordered list, replacing a prior result for
 * the same index rather than duplicating it — the decode loop can, in
 * principle, resend an index (e.g. a future retry), and window order in the
 * transcript must always follow `window_index`, not arrival order. */
function upsertWindow(
  windows: StreamingWindow[],
  incoming: StreamingWindow,
): StreamingWindow[] {
  const next = windows.filter((w) => w.window_index !== incoming.window_index);
  next.push(incoming);
  next.sort((a, b) => a.window_index - b.window_index);
  return next;
}

function sourcesLabel(sources: {
  mic: boolean;
  system_audio: boolean;
}): string {
  if (sources.mic && sources.system_audio) return "Mic + System audio";
  if (sources.mic) return "Mic only";
  if (sources.system_audio) return "System audio only";
  return "No audio source";
}

/** A window's text for display/export — the same `[unavailable]` marker the
 * live transcript shows for a fail-open window, so copy/export output
 * matches what's on screen rather than silently dropping or blanking a
 * failed span. */
function windowText(w: StreamingWindow): string {
  return w.outcome_ok ? w.text : "[unavailable]";
}

function plainTranscript(windows: StreamingWindow[]): string {
  return windows.map(windowText).join(" ").replace(/\s+/g, " ").trim();
}

function toMarkdown(title: string, windows: StreamingWindow[]): string {
  return `# ${title}\n\n${plainTranscript(windows)}\n`;
}

function fileNameFor(title: string): string {
  const slug = title.replace(/[^\w\- ]+/g, "").trim();
  return `${slug || "streaming-session"}.md`;
}

export function StreamingView({ onClose }: { onClose: () => void }) {
  const [sessions, setSessions] = useState<StreamingSessionSummary[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [activeTitle, setActiveTitle] = useState<string>("Streaming Session");
  const [windows, setWindows] = useState<StreamingWindow[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  const [sources, setSources] = useState<{
    mic: boolean;
    system_audio: boolean;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied">("idle");

  const refreshSessions = useCallback(async () => {
    setSessions(await listStreamingSessions());
  }, []);

  useEffect(() => {
    void refreshSessions();
  }, [refreshSessions]);

  useEffect(() => {
    let unlistenWindow: (() => void) | undefined;
    let unlistenSources: (() => void) | undefined;
    let unlistenEnded: (() => void) | undefined;
    let cancelled = false;

    void (async () => {
      const [w, s, e] = await Promise.all([
        onStreamingWindow((incoming) => {
          setActiveId((current) => {
            if (current === incoming.session_id) {
              setWindows((prev) => upsertWindow(prev, incoming));
            }
            return current;
          });
        }),
        onStreamingSources((incoming) => {
          setSources({
            mic: incoming.mic,
            system_audio: incoming.system_audio,
          });
        }),
        onStreamingSessionEnded(() => {
          setIsRunning(false);
          void refreshSessions();
        }),
      ]);
      if (cancelled) {
        w();
        s();
        e();
        return;
      }
      unlistenWindow = w;
      unlistenSources = s;
      unlistenEnded = e;
    })();

    return () => {
      cancelled = true;
      unlistenWindow?.();
      unlistenSources?.();
      unlistenEnded?.();
    };
  }, [refreshSessions]);

  const handleStart = useCallback(async () => {
    setError(null);
    setBusy(true);
    try {
      const summary = await startStreamingSession();
      setActiveId(summary.id);
      setActiveTitle(summary.title);
      setWindows([]);
      setSources(null);
      setCopyStatus("idle");
      setIsRunning(true);
      await refreshSessions();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [refreshSessions]);

  const handleStop = useCallback(async () => {
    setError(null);
    setBusy(true);
    try {
      await stopStreamingSession();
      // isRunning flips to false when streaming_session_ended fires, not
      // here — the backend, not this call returning, is the source of
      // truth for when the session actually finished.
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const handleOpen = useCallback(async (id: number) => {
    setError(null);
    try {
      const session = await openStreamingSession(id);
      setActiveId(id);
      setActiveTitle(session.title);
      setWindows(session.windows);
      setIsRunning(false);
      setSources(null);
      setCopyStatus("idle");
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleRename = useCallback(
    async (id: number, currentTitle: string) => {
      const title = window.prompt("Rename session", currentTitle);
      if (!title || title === currentTitle) return;
      try {
        await renameStreamingSession(id, title);
        if (activeId === id) setActiveTitle(title);
        await refreshSessions();
      } catch (e) {
        setError(String(e));
      }
    },
    [refreshSessions, activeId],
  );

  const handleDelete = useCallback(
    async (id: number, title: string) => {
      if (!window.confirm(`Delete "${title}"? This cannot be undone.`)) return;
      try {
        await deleteStreamingSession(id);
        setActiveId((current) => {
          if (current !== id) return current;
          setWindows([]);
          return null;
        });
        await refreshSessions();
      } catch (e) {
        setError(String(e));
      }
    },
    [refreshSessions],
  );

  const handleCopy = useCallback(async () => {
    setError(null);
    try {
      await navigator.clipboard.writeText(plainTranscript(windows));
      setCopyStatus("copied");
    } catch (e) {
      setError(String(e));
    }
  }, [windows]);

  const handleExport = useCallback(async () => {
    setError(null);
    try {
      await saveTextDialog(
        toMarkdown(activeTitle, windows),
        fileNameFor(activeTitle),
      );
    } catch (e) {
      setError(String(e));
    }
  }, [windows, activeTitle]);

  const hasText = windows.some((w) => windowText(w).length > 0);

  return (
    <div className="app streaming-view">
      <header className="wp-header" data-tauri-drag-region="deep">
        <div className="wp-header-lead">
          <div className="wp-header-left">
            <span
              className="wp-traffic-space"
              aria-hidden="true"
              data-tauri-drag-region
            />
            <AppLogo size={28} />
          </div>
          <div className="wp-title-group">
            <h1 className="wp-title">Streaming</h1>
          </div>
          <button
            type="button"
            className="wp-icon-btn"
            aria-label="Back to Meetings"
            onClick={onClose}
          >
            <Icon name="x" size={18} />
          </button>
        </div>
      </header>

      <div className="streaming-body">
        <aside className="streaming-sidebar" aria-label="Streaming sessions">
          <div className="streaming-controls">
            {!isRunning ? (
              <button
                type="button"
                className="wp-primary-btn"
                onClick={() => void handleStart()}
                disabled={busy}
              >
                <Icon name="play" size={16} />
                Start
              </button>
            ) : (
              <button
                type="button"
                className="wp-primary-btn"
                onClick={() => void handleStop()}
                disabled={busy}
              >
                <Icon name="square" size={16} />
                Stop
              </button>
            )}
            {sources && (
              <p className="streaming-sources" role="status">
                {sourcesLabel(sources)}
              </p>
            )}
          </div>

          {error && (
            <p role="alert" className="streaming-error">
              {error}
            </p>
          )}

          <ul className="streaming-session-list">
            {sessions.map((s) => (
              <li
                key={s.id}
                className={
                  s.id === activeId
                    ? "streaming-session-row streaming-session-row--active"
                    : "streaming-session-row"
                }
              >
                <button
                  type="button"
                  className="streaming-session-title"
                  onClick={() => void handleOpen(s.id)}
                  aria-current={s.id === activeId}
                >
                  {s.title}
                </button>
                <button
                  type="button"
                  className="wp-icon-btn wp-icon-btn--ghost"
                  aria-label={`Rename ${s.title}`}
                  onClick={() => void handleRename(s.id, s.title)}
                >
                  <Icon name="pencil" size={14} />
                </button>
                <button
                  type="button"
                  className="wp-icon-btn wp-icon-btn--ghost"
                  aria-label={`Delete ${s.title}`}
                  onClick={() => void handleDelete(s.id, s.title)}
                >
                  <Icon name="trash-2" size={14} />
                </button>
              </li>
            ))}
          </ul>
        </aside>

        <main className="streaming-transcript" aria-label="Live transcript">
          <div className="streaming-transcript-actions">
            <button
              type="button"
              className="streaming-action-btn"
              aria-label="Copy transcript"
              title="Copy transcript"
              onClick={() => void handleCopy()}
              disabled={!hasText}
            >
              <Icon name="check" size={16} />
              {copyStatus === "copied" ? "Copied" : "Copy"}
            </button>
            <button
              type="button"
              className="streaming-action-btn"
              aria-label="Export as Markdown"
              title="Export as Markdown"
              onClick={() => void handleExport()}
              disabled={!hasText}
            >
              <Icon name="download" size={16} />
              Export
            </button>
          </div>
          {windows.length === 0 ? (
            <p className="streaming-empty">
              {isRunning
                ? "Listening…"
                : "Start a session, or open one from the list."}
            </p>
          ) : (
            <div className="streaming-transcript-text">
              {windows.map((w) => (
                <span
                  key={w.window_index}
                  className={
                    w.outcome_ok
                      ? "streaming-window"
                      : "streaming-window streaming-window--failed"
                  }
                  title={`${formatClockTime(w.start_ms)}–${formatClockTime(w.end_ms)} (${w.language})`}
                >
                  {w.outcome_ok ? w.text : "[unavailable]"}{" "}
                </span>
              ))}
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
