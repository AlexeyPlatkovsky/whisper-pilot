import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  acceptStreamingPrettify,
  deleteStreamingSession,
  generateStreamingNotes,
  generateStreamingPrettify,
  listStreamingSessions,
  onStreamingSessionEnded,
  onStreamingSources,
  onStreamingWindow,
  openStreamingSession,
  renameStreamingSession,
  revertStreamingPrettify,
  saveTextDialog,
  startStreamingSession,
  stopStreamingSession,
  type StreamingNotes,
  type StreamingSessionSummary,
  type StreamingWindow,
} from "./ipc";
import { AppLogo, Icon } from "./Icon";
import { ActionIcon } from "./ActionIcon";
import { CopyButton } from "./CopyButton";
import { ModeToggle } from "./ModeToggle";
import { formatElapsedClock } from "./format";
import { computeWordDiff } from "./diff";
import { groupWindowsIntoParagraphs } from "./paragraphs";
import { StreamingSessionRow } from "./StreamingSessionRow";
import {
  fileNameFor,
  formatClockTime,
  plainTranscript,
  sourcesLabel,
  toMarkdown,
  upsertWindow,
  windowText,
} from "./streamingText";
import {
  resolveStreamingRowStatus,
  resolveStreamingWidgetStatus,
} from "./streamingStatus";

export function StreamingView({
  onClose,
  onOpenSettings,
}: {
  onClose: () => void;
  onOpenSettings: () => void;
}) {
  const [sessions, setSessions] = useState<StreamingSessionSummary[]>([]);
  const [sessionSearch, setSessionSearch] = useState("");
  const [activeId, setActiveId] = useState<number | null>(null);
  const [activeTitle, setActiveTitle] = useState<string>("Streaming Session");
  const [windows, setWindows] = useState<StreamingWindow[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [sources, setSources] = useState<{
    mic: boolean;
    system_audio: boolean;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const startTimeRef = useRef<number | null>(null);
  const [craftingId, setCraftingId] = useState<number | null>(null);
  const [notes, setNotes] = useState<StreamingNotes | null>(null);
  const [craftFailed, setCraftFailed] = useState(false);
  const [prettifyingId, setPrettifyingId] = useState<number | null>(null);
  const [prettifyFailed, setPrettifyFailed] = useState(false);
  const [prettifiedText, setPrettifiedText] = useState<string | null>(null);
  const [pendingPrettify, setPendingPrettify] = useState<{
    original: string;
    cleaned: string;
  } | null>(null);
  // In-app modals (matching App.tsx's Meeting rename/delete pattern) rather
  // than window.prompt/window.confirm — Tauri's WKWebView doesn't reliably
  // wire up the native JS dialog delegate, so those silently no-op instead
  // of showing anything.
  const [renameTarget, setRenameTarget] = useState<{
    id: number;
    title: string;
  } | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [renameError, setRenameError] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{
    id: number;
    title: string;
  } | null>(null);
  // Read after an await to avoid acting on a stale closure once the user has
  // switched sessions — plain state would still hold the id captured when
  // the async handler started.
  const activeIdRef = useRef<number | null>(null);
  useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);

  // Scoped to the open session, so switching sessions can't paint a stale
  // crafting/prettifying state onto the newly opened one.
  const isCraftingActive = craftingId !== null && craftingId === activeId;
  const isPrettifyingActive =
    prettifyingId !== null && prettifyingId === activeId;

  // Derived, not stored, so it can't drift from Start/Stop; isRunning wins
  // over busy so a Stop-in-flight still reads as On Air.
  const widgetStatus = isRunning
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
              : "ready";
  const widget = resolveStreamingWidgetStatus(widgetStatus);
  const filteredSessions = useMemo(() => {
    const query = sessionSearch.trim().toLocaleLowerCase();
    if (query.length < 3) return sessions;
    return sessions.filter((session) =>
      session.title.toLocaleLowerCase().includes(query),
    );
  }, [sessionSearch, sessions]);

  // Recomputed from Date.now() each tick, not incremented, so a throttled
  // setInterval can't drift the displayed value. Shared by On Air, Crafting,
  // and Prettifying — mutually exclusive states (each is disabled while any
  // other is active).
  useEffect(() => {
    if (!isRunning && !isCraftingActive && !isPrettifyingActive) return;
    startTimeRef.current = Date.now();
    setElapsed(0);
    const id = setInterval(() => {
      setElapsed(Math.floor((Date.now() - startTimeRef.current!) / 1000));
    }, 1000);
    return () => clearInterval(id);
  }, [isRunning, isCraftingActive, isPrettifyingActive]);

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

  // `resumeId` is the session to continue capturing into, or null for a
  // brand-new one. Either way, any LLM-result state (notes/prettified/failed
  // flags) is stale the moment new audio starts arriving, so it's cleared in
  // both cases — only `windows` survives a resume, since preserving the
  // session's transcript-so-far is the whole point of resuming into it.
  const startSession = useCallback(
    async (resumeId: number | null) => {
      setError(null);
      setBusy(true);
      try {
        const summary = await startStreamingSession(resumeId ?? undefined);
        setActiveId(summary.id);
        setActiveTitle(summary.title);
        if (resumeId === null) setWindows([]);
        setSources(null);
        setNotes(null);
        setCraftFailed(false);
        setPrettifiedText(null);
        setPrettifyFailed(false);
        setPendingPrettify(null);
        setIsRunning(true);
        await refreshSessions();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [refreshSessions],
  );

  // Reads as "continue what's on screen": resumes the currently open session
  // when it's a past, stopped one, otherwise starts fresh — bound to the
  // header's Start icon.
  const handleStart = useCallback(() => {
    const resumeId = activeId !== null && !isRunning ? activeId : null;
    return startSession(resumeId);
  }, [startSession, activeId, isRunning]);

  // Always starts a brand-new session regardless of what's currently open —
  // bound to the "+"/New icon, which is unambiguously "create", not
  // "continue".
  const handleStartNew = useCallback(() => startSession(null), [startSession]);

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
      setNotes(session.notes ?? null);
      setCraftFailed(false);
      setPrettifiedText(session.prettified_text ?? null);
      setPrettifyFailed(false);
      setPendingPrettify(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  function openRename(id: number, title: string) {
    setRenameTarget({ id, title });
    setRenameDraft(title);
    setRenameError(null);
  }

  function closeRename() {
    setRenameTarget(null);
    setRenameError(null);
  }

  const submitRename = useCallback(
    async (event: React.FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      if (!renameTarget) return;
      const title = renameDraft.trim();
      if (!title) {
        setRenameError("Session label is required");
        return;
      }
      if (Array.from(title).length > 120) {
        setRenameError("Session label must be 120 characters or fewer");
        return;
      }
      setError(null);
      try {
        await renameStreamingSession(renameTarget.id, title);
        if (activeId === renameTarget.id) setActiveTitle(title);
        await refreshSessions();
        closeRename();
      } catch (e) {
        setError(String(e));
      }
    },
    [renameTarget, renameDraft, activeId, refreshSessions],
  );

  function openDelete(id: number, title: string) {
    setDeleteTarget({ id, title });
  }

  const confirmDelete = useCallback(async () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setError(null);
    try {
      await deleteStreamingSession(target.id);
      setActiveId((current) => {
        if (current !== target.id) return current;
        setWindows([]);
        setNotes(null);
        setCraftFailed(false);
        setPrettifiedText(null);
        setPrettifyFailed(false);
        setPendingPrettify(null);
        return null;
      });
      await refreshSessions();
      setDeleteTarget(null);
    } catch (e) {
      setError(String(e));
    }
  }, [deleteTarget, refreshSessions]);

  // Once accepted, the cleaned text is what gets copied/exported — that's
  // the point of prettifying.
  const exportText = prettifiedText ?? plainTranscript(windows);

  const handleExport = useCallback(async () => {
    setError(null);
    try {
      await saveTextDialog(
        toMarkdown(activeTitle, exportText),
        fileNameFor(activeTitle),
      );
    } catch (e) {
      setError(String(e));
    }
  }, [exportText, activeTitle]);

  const handleCraft = useCallback(async () => {
    // c8 ignore next -- the action is not rendered until an active session exists.
    if (activeId === null) return;
    const id = activeId;
    setError(null);
    setCraftFailed(false);
    setCraftingId(id);
    try {
      const session = await generateStreamingNotes(id);
      if (activeIdRef.current === id) {
        setNotes(session.notes ?? null);
      }
    } catch (e) {
      if (activeIdRef.current === id) {
        setError(String(e));
        setCraftFailed(true);
      }
    } finally {
      setCraftingId((current) => (current === id ? null : current));
    }
  }, [activeId]);

  const handlePrettify = useCallback(async () => {
    // c8 ignore next -- the action is not rendered until an active session exists.
    if (activeId === null) return;
    const id = activeId;
    const original = plainTranscript(windows);
    setError(null);
    setPrettifyFailed(false);
    setPrettifyingId(id);
    try {
      const cleaned = await generateStreamingPrettify(id);
      if (activeIdRef.current === id) {
        setPendingPrettify({ original, cleaned });
      }
    } catch (e) {
      if (activeIdRef.current === id) {
        setError(String(e));
        setPrettifyFailed(true);
      }
    } finally {
      setPrettifyingId((current) => (current === id ? null : current));
    }
  }, [activeId, windows]);

  const handleAcceptPrettify = useCallback(async () => {
    // c8 ignore next -- Accept is only rendered while a review is pending.
    if (activeId === null || !pendingPrettify) return;
    const id = activeId;
    const text = pendingPrettify.cleaned;
    setError(null);
    try {
      const session = await acceptStreamingPrettify(id, text);
      if (activeIdRef.current === id) {
        setPrettifiedText(session.prettified_text ?? null);
        setPendingPrettify(null);
      }
    } catch (e) {
      if (activeIdRef.current === id) {
        setError(String(e));
      }
    }
  }, [activeId, pendingPrettify]);

  const handleCancelPrettify = useCallback(() => {
    setPendingPrettify(null);
  }, []);

  const handleRevertPrettify = useCallback(async () => {
    // c8 ignore next -- Revert is only rendered while accepted text exists.
    if (activeId === null || prettifiedText === null) return;
    const id = activeId;
    setError(null);
    try {
      const session = await revertStreamingPrettify(id);
      if (activeIdRef.current === id) {
        setPrettifiedText(session.prettified_text ?? null);
      }
    } catch (e) {
      if (activeIdRef.current === id) setError(String(e));
    }
  }, [activeId, prettifiedText]);

  const hasText = windows.some((w) => windowText(w).length > 0);
  // Craft/Prettify need real decoded content, unlike Copy/Export's hasText —
  // a session with only fail-open windows has hasText=true (from the
  // "[unavailable]" placeholder) but nothing for the LLM to work from.
  const hasCraftableText = windows.some(
    (w) => w.outcome_ok && w.text.length > 0,
  );
  const canPrettify =
    isRunning || !hasCraftableText || isCraftingActive || isPrettifyingActive;

  const windowCountLabel =
    windows.length === 0
      ? "No windows"
      : `${windows.length} window${windows.length === 1 ? "" : "s"}`;
  const durationLabel =
    windows.length === 0
      ? "—"
      : formatClockTime(windows[windows.length - 1].end_ms);

  const activeBusy = isRunning || isCraftingActive || isPrettifyingActive;

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
            <div className="wp-action-group">
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="Toggle sidebar"
                aria-pressed={sidebarOpen}
                onClick={() => setSidebarOpen((v) => !v)}
              >
                <Icon name="panel-left" size={18} />
              </button>
              <span className="wp-sep" />
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="New streaming session"
                title="New streaming session"
                onClick={() => void handleStartNew()}
                disabled={busy || isRunning}
              >
                <Icon name="plus" size={18} />
              </button>
              <span className="wp-sep" />
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="Settings"
                onClick={onOpenSettings}
              >
                <Icon name="settings" size={18} />
              </button>
            </div>
          </div>

          <div className="wp-title-group">
            <h1 className="wp-title">{activeTitle}</h1>
            <button
              type="button"
              className="wp-icon-btn wp-icon-btn--ghost"
              aria-label="Rename session"
              onClick={() =>
                activeId !== null && openRename(activeId, activeTitle)
              }
              disabled={activeId === null || activeBusy}
            >
              <Icon name="pencil" size={14} />
            </button>
            <button
              type="button"
              className="wp-icon-btn wp-icon-btn--ghost"
              aria-label="Delete session"
              onClick={() =>
                activeId !== null && openDelete(activeId, activeTitle)
              }
              disabled={activeId === null || activeBusy}
            >
              <Icon name="trash-2" size={14} />
            </button>
          </div>
        </div>

        <div className="wp-header-right">
          <div className="wp-status" role="status">
            <Icon
              name={widget.icon}
              size={14}
              className={
                widget.spinning
                  ? `wp-spin wp-tone--${widget.tone}`
                  : `wp-tone--${widget.tone}`
              }
            />
            <span className={`wp-status-label wp-tone--${widget.tone}`}>
              {widget.label}
            </span>
            {widget.showTimer && (
              // aria-hidden: a role="status" live region re-announces on
              // every accessible-tree change: without this, the ticking
              // timer would spam a screen reader once per second.
              <span className="wp-status-timer" aria-hidden="true">
                {formatElapsedClock(elapsed)}
              </span>
            )}
          </div>

          <div className="wp-action-group">
            <ActionIcon
              icon="play"
              label={activeId !== null && !isRunning ? "Resume" : "Start"}
              onClick={() => void handleStart()}
              disabled={busy || isRunning}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="square"
              label="Stop"
              onClick={() => void handleStop()}
              disabled={busy || !isRunning}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="sparkles"
              label="Craft MFU notes"
              accent
              onClick={() => void handleCraft()}
              disabled={canPrettify}
            />
            <span className="wp-sep" />
            <CopyButton
              text={exportText}
              resetKey={activeId}
              onError={setError}
              onCopied={() => setError(null)}
              disabled={!hasText}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="download"
              label="Export as Markdown"
              onClick={() => void handleExport()}
              disabled={!hasText}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="trash-2"
              label="Delete active session"
              onClick={() =>
                activeId !== null && openDelete(activeId, activeTitle)
              }
              disabled={activeId === null || isRunning}
            />
          </div>
        </div>
      </header>

      <div className="wp-info-bar">
        <div className="wp-info-left">
          <span className="wp-info-label">Audio Source:</span>
          <span className="wp-file-chip">
            <Icon name="mic" size={14} />
            {sources ? sourcesLabel(sources) : "No audio source"}
          </span>
        </div>
        <div className="wp-info-right">
          <span className="wp-info-meta">
            <Icon name="globe" size={14} />
            Auto-detect (EN/RU/TR)
          </span>
          <span className="wp-info-meta">{durationLabel}</span>
        </div>
      </div>

      <div className="wp-main">
        {sidebarOpen && (
          <aside className="wp-sidebar">
            <ModeToggle
              mode="streaming"
              onSelectMeeting={onClose}
              onSelectStreaming={() => {}}
            />
            <div className="wp-search">
              <Icon name="search" size={16} />
              <input
                type="search"
                className="wp-search-input"
                placeholder="Search sessions..."
                aria-label="Search sessions"
                value={sessionSearch}
                onChange={(event) => setSessionSearch(event.target.value)}
              />
            </div>

            {sessions.length === 0 ? (
              <p className="wp-info-muted">No sessions yet</p>
            ) : filteredSessions.length === 0 ? (
              <p className="wp-info-muted">No matches</p>
            ) : (
              <ul className="wp-meeting-list" role="list">
                {filteredSessions.map((s) => (
                  <StreamingSessionRow
                    key={s.id}
                    title={s.title}
                    when={new Date(s.created_at_ms).toLocaleDateString()}
                    dur={formatClockTime(
                      Math.max(0, s.updated_at_ms - s.created_at_ms),
                    )}
                    status={
                      s.id === activeId
                        ? widget
                        : resolveStreamingRowStatus(s.status)
                    }
                    selected={activeId === s.id}
                    onSelect={() => void handleOpen(s.id)}
                    onRename={() => openRename(s.id, s.title)}
                    onDelete={() => openDelete(s.id, s.title)}
                  />
                ))}
              </ul>
            )}
          </aside>
        )}

        <section className="wp-workspace">
          <div className="wp-transcript-panel">
            <div className="wp-transcript-header">
              <div className="wp-transcript-title-group">
                <h2 className="wp-transcript-title">Live Transcript</h2>
                <span className="wp-transcript-meta">{windowCountLabel}</span>
              </div>
              <div className="wp-transcript-actions">
                <Icon name="pencil" size={14} />
                <span>Editable</span>
                <span className="wp-sep" />
                {pendingPrettify && (
                  <>
                    <button
                      type="button"
                      className="wp-icon-btn"
                      aria-label="Accept Prettify"
                      title="Accept"
                      onClick={() => void handleAcceptPrettify()}
                    >
                      <Icon
                        name="check"
                        size={15}
                        className="wp-tone--finished"
                      />
                    </button>
                    <button
                      type="button"
                      className="wp-icon-btn"
                      aria-label="Cancel Prettify"
                      title="Cancel"
                      onClick={handleCancelPrettify}
                    >
                      <Icon name="x" size={15} />
                    </button>
                  </>
                )}
                {!pendingPrettify && prettifiedText !== null && (
                  <button
                    type="button"
                    className="wp-icon-btn"
                    aria-label="Cancel Prettify"
                    title="Revert to original transcript"
                    onClick={() => void handleRevertPrettify()}
                  >
                    <Icon name="x" size={15} />
                  </button>
                )}
                <button
                  type="button"
                  className="wp-icon-btn wp-icon-btn--accent"
                  aria-label="Prettify transcript"
                  title="Prettify transcript"
                  onClick={() => void handlePrettify()}
                  disabled={canPrettify || pendingPrettify !== null}
                >
                  <Icon name="wand-sparkles" size={15} />
                </button>
              </div>
            </div>
            <div className="wp-separator" />

            <div className="wp-transcript-content">
              {error && (
                <div className="wp-notice wp-notice--error" role="alert">
                  {error}
                </div>
              )}

              {windows.length === 0 ? (
                <div className="wp-empty">
                  <p>
                    {isRunning
                      ? "Listening…"
                      : "Start a session, or open one from the list."}
                  </p>
                </div>
              ) : pendingPrettify ? (
                <div className="streaming-transcript-text">
                  {computeWordDiff(
                    pendingPrettify.original,
                    pendingPrettify.cleaned,
                  ).map((span, i) => {
                    // <del>/<ins> so a screen reader announces the change,
                    // not just strikethrough/color a sighted user sees.
                    if (span.type === "del") {
                      return (
                        <del key={i} className="diff-del">
                          {span.text}
                        </del>
                      );
                    }
                    if (span.type === "add") {
                      return (
                        <ins key={i} className="diff-add">
                          {span.text}
                        </ins>
                      );
                    }
                    return <span key={i}>{span.text}</span>;
                  })}
                </div>
              ) : prettifiedText !== null ? (
                <div className="streaming-transcript-text">
                  {prettifiedText}
                </div>
              ) : (
                <div className="streaming-transcript-text">
                  {groupWindowsIntoParagraphs(windows).map((paragraph) => (
                    <p
                      key={paragraph[0].window_index}
                      className="streaming-paragraph"
                    >
                      {paragraph.map((w) => (
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
                    </p>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* MFU (summary) panel */}
          <aside className="wp-mfu">
            {notes ? (
              <div className="wp-mfu-notes">
                {notes.summary && (
                  <section className="wp-mfu-section">
                    <h3 className="wp-mfu-heading">Summary</h3>
                    <p className="wp-mfu-text">{notes.summary}</p>
                  </section>
                )}
                {notes.decisions && (
                  <section className="wp-mfu-section">
                    <h3 className="wp-mfu-heading">Decisions</h3>
                    <p className="wp-mfu-text">{notes.decisions}</p>
                  </section>
                )}
                {notes.action_items && (
                  <section className="wp-mfu-section">
                    <h3 className="wp-mfu-heading">Action Items</h3>
                    <p className="wp-mfu-text">{notes.action_items}</p>
                  </section>
                )}
                {notes.open_questions && (
                  <section className="wp-mfu-section">
                    <h3 className="wp-mfu-heading">Open Questions</h3>
                    <p className="wp-mfu-text">{notes.open_questions}</p>
                  </section>
                )}
                {notes.participants && (
                  <section className="wp-mfu-section">
                    <h3 className="wp-mfu-heading">Participants</h3>
                    <p className="wp-mfu-text">{notes.participants}</p>
                  </section>
                )}
              </div>
            ) : (
              <div className="wp-mfu-placeholder">
                <div className="wp-mfu-icon-frame">
                  <Icon name="sparkles" size={32} />
                </div>
                <p className="wp-mfu-title">Run MFU Craft</p>
                <p className="wp-mfu-subtitle">
                  Generate summary, decisions, action items, and open questions.
                </p>
              </div>
            )}
          </aside>
        </section>
      </div>

      {renameTarget && (
        <div className="modal-overlay">
          <form
            className="modal-panel confirm-modal"
            role="dialog"
            aria-modal="true"
            aria-label="Rename session"
            onSubmit={submitRename}
            onKeyDown={(event) => {
              if (event.key === "Escape") closeRename();
            }}
          >
            <div className="modal-header">
              <span className="modal-title">Rename session</span>
            </div>
            <label htmlFor="streaming-session-label">Session label</label>
            <input
              id="streaming-session-label"
              type="text"
              value={renameDraft}
              autoFocus
              onFocus={(event) => event.currentTarget.select()}
              onChange={(event) => {
                setRenameDraft(event.target.value);
                setRenameError(null);
              }}
              aria-invalid={renameError ? true : undefined}
            />
            {renameError && <p role="alert">{renameError}</p>}
            <div className="confirm-actions">
              <button type="button" onClick={closeRename}>
                Cancel
              </button>
              <button type="submit">Save</button>
            </div>
          </form>
        </div>
      )}

      {deleteTarget && (
        <div className="modal-overlay">
          <div
            className="modal-panel confirm-modal"
            role="alertdialog"
            aria-modal="true"
            aria-label={`Delete ${deleteTarget.title}`}
            onKeyDown={(event) => {
              if (event.key === "Escape") setDeleteTarget(null);
            }}
          >
            <div className="modal-header">
              <span className="modal-title">Delete {deleteTarget.title}?</span>
            </div>
            <p className="confirm-warning">
              This permanently removes the session and its transcript.
            </p>
            <div className="confirm-actions">
              <button type="button" onClick={() => setDeleteTarget(null)}>
                Cancel
              </button>
              <button type="button" onClick={() => void confirmDelete()}>
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
