import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  acceptRecorderPolish,
  clearRecorderTranscript,
  collapseToBubble,
  createRecorderDraft,
  deleteRecorderSession,
  exportRecorderWav,
  generateRecorderPolish,
  getLiveCaptureSnapshot,
  getMicrophonePermissionStatus,
  listRecorderSessions,
  onLiveCaptureState,
  onRecorderError,
  onRecorderPartial,
  onRecorderSegmentCommitted,
  onRecorderSessionChanged,
  openRecorderSession,
  recoverRecorderSession,
  renameRecorderSession,
  requestMicrophonePermission,
  revertRecorderPolish,
  saveTextDialog,
  startRecorderSession,
  stopRecorderSession,
  updateRecorderSegment,
  type LiveCaptureSnapshot,
  type RecorderSegment,
  type RecorderSession,
  type RecorderSessionSummary,
  type RecorderStatus,
} from "./ipc";
import { reconcileLiveCaptureSnapshot } from "./liveCaptureState";
import { ModeToggle } from "./ModeToggle";
import { AppLogo, Icon } from "./Icon";
import { ActionIcon } from "./ActionIcon";
import { CopyButton } from "./CopyButton";
import { StreamingSessionRow } from "./StreamingSessionRow";
import type { StreamingStatusView } from "./streamingStatus";
import { formatDuration, formatElapsedClock } from "./format";
import { stabilizePartial, type StablePartial } from "./partialStability";
import { groupWindowsIntoParagraphs } from "./paragraphs";

const AUTOSCROLL_RESUME_THRESHOLD_PX = 48;

function persistedStatus(
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

function recorderCaptureStatus(
  snapshot: LiveCaptureSnapshot | null,
): StreamingStatusView | null {
  if (snapshot?.source === "recorder") {
    if (snapshot.phase === "starting") {
      return {
        tone: "unknown",
        label: "Starting",
        icon: "refresh-cw",
        spinning: true,
        showTimer: false,
        statusKey: "starting",
      };
    }
    if (snapshot.phase === "capturing") {
      return persistedStatus("recording");
    }
    if (snapshot.phase === "stopping") {
      return {
        tone: "unknown",
        label: "Stopping · Finalizing",
        icon: "refresh-cw",
        spinning: true,
        showTimer: false,
        statusKey: "starting",
      };
    }
    if (snapshot.phase === "error") {
      return {
        tone: "error",
        label: "Error",
        icon: "alert-circle",
        spinning: false,
        showTimer: false,
        statusKey: "error",
      };
    }
  }
  return null;
}

function recorderStatus(
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
  if (active?.is_draft) {
    return persistedStatus(active.status, true);
  }
  if (active !== null && snapshot?.session_id === active.id) {
    const captureStatus = recorderCaptureStatus(snapshot);
    if (captureStatus) return captureStatus;
  }
  return active
    ? persistedStatus(active.status)
    : {
        tone: "finished",
        label: snapshot ? "Ready" : "Loading",
        icon: snapshot ? "check" : "refresh-cw",
        spinning: snapshot === null,
        showTimer: false,
        statusKey: snapshot ? "ready" : "unknown",
      };
}

interface RecorderViewProps {
  onSelectMeeting: () => void;
  onSelectStreaming: () => void;
  onOpenSettings: () => void;
  meetingTranscriptionActive: boolean;
  recorderStartPending?: boolean;
  onRecorderStartPendingChange?: (pending: boolean) => void;
}

export function RecorderView({
  onSelectMeeting,
  onSelectStreaming,
  onOpenSettings,
  meetingTranscriptionActive,
  recorderStartPending = false,
  onRecorderStartPendingChange = () => {},
}: RecorderViewProps) {
  const [sessions, setSessions] = useState<RecorderSessionSummary[]>([]);
  const [active, setActive] = useState<RecorderSession | null>(null);
  const [snapshot, setSnapshot] = useState<LiveCaptureSnapshot | null>(null);
  const [partial, setPartial] = useState("");
  const [partialDisplay, setPartialDisplay] = useState<StablePartial>({
    stable: "",
    unstable: "",
  });
  const [error, setError] = useState<string | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [search, setSearch] = useState("");
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
  const [clearPending, setClearPending] = useState(false);
  const [polishBusy, setPolishBusy] = useState(false);
  const [creatingDraft, setCreatingDraft] = useState(false);
  const [startPending, setStartPending] = useState(false);
  const [segmentEdits, setSegmentEdits] = useState<Record<number, string>>({});
  const [elapsed, setElapsed] = useState(0);
  const partialCursor = useRef<{ sessionId: number | null; revision: number }>({
    sessionId: null,
    revision: -1,
  });
  const activeId = useRef<number | null>(null);
  const activeStatus = useRef<RecorderStatus | null>(null);
  const previousPartial = useRef("");
  const transcriptScrollRef = useRef<HTMLDivElement | null>(null);
  const autoScrollEnabledRef = useRef(true);
  const startPendingRef = useRef(false);
  const segmentEditVersions = useRef<Map<number, number>>(new Map());
  const segmentSaveChains = useRef<Map<number, Promise<void>>>(new Map());

  const resetPartial = useCallback(() => {
    previousPartial.current = "";
    setPartial("");
    setPartialDisplay({ stable: "", unstable: "" });
  }, []);

  const upsertSession = useCallback((session: RecorderSessionSummary) => {
    setSessions((current) => [
      session,
      ...current.filter((item) => item.id !== session.id),
    ]);
  }, []);

  const displaySession = useCallback(
    (session: RecorderSession) => {
      if (activeId.current !== session.id) {
        autoScrollEnabledRef.current = true;
        setSegmentEdits({});
      }
      if (activeId.current !== session.id || session.status !== "recording") {
        resetPartial();
        partialCursor.current = { sessionId: session.id, revision: -1 };
      }
      activeId.current = session.id;
      activeStatus.current = session.status;
      setActive(session);
    },
    [resetPartial],
  );

  useEffect(() => {
    let cancelled = false;
    void listRecorderSessions()
      .then((items) => {
        if (!cancelled) setSessions(items);
      })
      .catch((reason) => {
        if (!cancelled) setError(String(reason));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    const register = async () => {
      unlisteners.push(
        await onLiveCaptureState((next) => {
          if (!cancelled) {
            setSnapshot((current) =>
              reconcileLiveCaptureSnapshot(current, next),
            );
          }
        }),
        await onRecorderSessionChanged((session) => {
          if (cancelled) return;
          upsertSession(session);
          if (
            activeId.current === session.id ||
            session.status === "recording"
          ) {
            displaySession(session);
          }
        }),
        await onRecorderSegmentCommitted((segment) => {
          if (cancelled || activeId.current !== segment.session_id) return;
          setActive((current) => {
            if (!current || current.id !== segment.session_id) return current;
            const withoutSame = current.segments.filter(
              (item) => item.id !== segment.id,
            );
            return { ...current, segments: [...withoutSame, segment] };
          });
          resetPartial();
        }),
        await onRecorderPartial((next) => {
          if (
            cancelled ||
            activeId.current !== next.session_id ||
            activeStatus.current !== "recording"
          ) {
            return;
          }
          const cursor = partialCursor.current;
          const previousRevision =
            cursor.sessionId === next.session_id ? cursor.revision : -1;
          if (next.revision <= previousRevision) return;
          partialCursor.current = {
            sessionId: next.session_id,
            revision: next.revision,
          };
          setPartialDisplay(
            stabilizePartial(previousPartial.current, next.text),
          );
          previousPartial.current = next.text;
          setPartial(next.text);
        }),
        await onRecorderError((next) => {
          if (
            !cancelled &&
            (next.session_id === undefined ||
              next.session_id === activeId.current)
          ) {
            setError(next.message);
          }
        }),
      );
      const initial = await getLiveCaptureSnapshot();
      if (!cancelled) {
        setSnapshot((current) =>
          reconcileLiveCaptureSnapshot(current, initial),
        );
      }
    };
    void register();
    return () => {
      cancelled = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [displaySession, resetPartial, upsertSession]);

  useEffect(() => {
    const sessionId = snapshot?.session_id;
    if (
      snapshot?.source !== "recorder" ||
      sessionId === null ||
      sessionId === undefined
    ) {
      return;
    }
    let cancelled = false;
    void openRecorderSession(sessionId)
      .then((session) => {
        if (!cancelled && session.id === sessionId) {
          displaySession(session);
          upsertSession(session);
        }
      })
      .catch((reason) => {
        if (!cancelled) setError(String(reason));
      });
    return () => {
      cancelled = true;
    };
  }, [
    displaySession,
    snapshot?.generation,
    snapshot?.session_id,
    snapshot?.source,
    upsertSession,
  ]);

  const ownsCapture =
    snapshot?.source === "recorder" &&
    snapshot.phase !== "idle" &&
    snapshot.phase !== "error";
  const captureBusy =
    snapshot === null ||
    meetingTranscriptionActive ||
    (snapshot.phase !== "idle" && snapshot.phase !== "error");
  const canStart = !captureBusy && !startPending && !recorderStartPending;
  const canStop = ownsCapture && snapshot?.phase === "capturing";

  const handleTranscriptScroll = useCallback(
    (event: React.UIEvent<HTMLDivElement>) => {
      const container = event.currentTarget;
      const distanceFromBottom = Math.max(
        0,
        container.scrollHeight - container.clientHeight - container.scrollTop,
      );
      autoScrollEnabledRef.current =
        distanceFromBottom <= AUTOSCROLL_RESUME_THRESHOLD_PX;
    },
    [],
  );

  useLayoutEffect(() => {
    const container = transcriptScrollRef.current;
    if (!ownsCapture || !autoScrollEnabledRef.current || !container) return;
    container.scrollTop = container.scrollHeight;
  }, [active?.segments, ownsCapture, partial]);

  useEffect(() => {
    if (!canStop) {
      setElapsed(Math.floor((active?.duration_ms ?? 0) / 1_000));
      return;
    }
    const startedAt = Date.now() - (active?.duration_ms ?? 0);
    const update = () =>
      setElapsed(Math.floor((Date.now() - startedAt) / 1_000));
    update();
    const timer = window.setInterval(update, 1_000);
    return () => window.clearInterval(timer);
  }, [active?.duration_ms, canStop]);

  async function openSession(id: number) {
    setError(null);
    try {
      displaySession(await openRecorderSession(id));
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function newRecording() {
    if (ownsCapture || creatingDraft) return;
    setError(null);
    setCreatingDraft(true);
    try {
      const draft = await createRecorderDraft();
      autoScrollEnabledRef.current = true;
      displaySession(draft);
      upsertSession(draft);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setCreatingDraft(false);
    }
  }

  async function start() {
    if (startPendingRef.current) return;
    startPendingRef.current = true;
    setStartPending(true);
    onRecorderStartPendingChange(true);
    setError(null);
    try {
      let permission = await getMicrophonePermissionStatus();
      if (permission === "not_determined") {
        permission = await requestMicrophonePermission();
      }
      if (permission !== "authorized") {
        setError(`Microphone permission is ${permission.replace("_", " ")}.`);
        return;
      }
      const session = await startRecorderSession(
        active?.is_draft ? active.id : undefined,
      );
      displaySession(session);
      upsertSession(session);
    } catch (reason) {
      setError(String(reason));
    } finally {
      startPendingRef.current = false;
      setStartPending(false);
      onRecorderStartPendingChange(false);
    }
  }

  async function stop() {
    setError(null);
    try {
      await stopRecorderSession();
    } catch (reason) {
      setError(String(reason));
    }
  }

  function openRename(id: number, title: string) {
    setRenameTarget({ id, title });
    setRenameDraft(title);
    setRenameError(null);
  }

  async function saveRename(event?: React.FormEvent) {
    event?.preventDefault();
    if (!renameTarget) return;
    const title = renameDraft.trim();
    if (!title) {
      setRenameError("Title cannot be empty.");
      return;
    }
    try {
      const renamed = await renameRecorderSession(renameTarget.id, title);
      upsertSession(renamed);
      if (activeId.current === renamed.id) displaySession(renamed);
      setRenameTarget(null);
    } catch (reason) {
      setRenameError(String(reason));
    }
  }

  async function deleteSession(target: { id: number; title: string }) {
    const id = target.id;
    setError(null);
    try {
      await deleteRecorderSession(id);
      setSessions((items) => items.filter((item) => item.id !== id));
      if (activeId.current === id) {
        activeId.current = null;
        activeStatus.current = null;
        setActive(null);
        resetPartial();
      }
      setDeleteTarget(null);
    } catch (reason) {
      setError(String(reason));
      setDeleteTarget(null);
      if (activeId.current === id) {
        try {
          const refreshed = await openRecorderSession(id);
          displaySession(refreshed);
          upsertSession(refreshed);
        } catch {
          // Keep the original actionable cleanup error visible.
        }
      }
    }
  }

  async function commitEdit(segment: RecorderSegment, text: string) {
    if (!active) return;
    const sessionId = active.id;
    const revision = segmentEditVersions.current.get(segment.id) ?? 0;
    const previous =
      segmentSaveChains.current.get(segment.id) ?? Promise.resolve();
    const request = previous
      .catch(() => {})
      .then(() => updateRecorderSegment(sessionId, segment.id, text));
    const tail = request.then(
      () => {},
      () => {},
    );
    segmentSaveChains.current.set(segment.id, tail);
    try {
      const updated = await request;
      setActive((current) =>
        current && current.id === sessionId
          ? {
              ...current,
              segments: current.segments.map((item) =>
                item.id === segment.id ? updated : item,
              ),
            }
          : current,
      );
      if (segmentEditVersions.current.get(segment.id) === revision) {
        setSegmentEdits((current) => {
          const next = { ...current };
          delete next[segment.id];
          return next;
        });
      }
    } catch (reason) {
      if (
        activeId.current === sessionId &&
        segmentEditVersions.current.get(segment.id) === revision
      ) {
        setSegmentEdits((current) => {
          const next = { ...current };
          delete next[segment.id];
          return next;
        });
        setError(String(reason));
      }
    } finally {
      if (segmentSaveChains.current.get(segment.id) === tail) {
        segmentSaveChains.current.delete(segment.id);
      }
    }
  }

  const transcriptParagraphs = useMemo(
    () =>
      groupWindowsIntoParagraphs(
        (active?.segments ?? []).map((segment) => ({
          ...segment,
          text: segmentEdits[segment.id] ?? segment.text,
          outcome_ok: true,
        })),
      ),
    [active?.segments, segmentEdits],
  );
  const rawTranscript = useMemo(
    () =>
      transcriptParagraphs
        .map((paragraph) =>
          paragraph
            .map((segment) => segment.text.trim())
            .filter(Boolean)
            .join(" "),
        )
        .filter(Boolean)
        .join("\n\n"),
    [transcriptParagraphs],
  );
  const transcript = active?.polished_text ?? rawTranscript;
  const hasPendingSegmentEdits = Object.keys(segmentEdits).length > 0;
  const destructiveDisabled =
    active?.status === "recording" ||
    active?.status === "finalizing" ||
    (ownsCapture && active?.id === snapshot?.session_id);
  const widget = recorderStatus(active, snapshot, polishBusy);
  const snapshotWidget = recorderCaptureStatus(snapshot);
  const effectiveError =
    error ??
    (snapshot?.source === "recorder" &&
    snapshot.phase === "error" &&
    snapshot.session_id === active?.id
      ? snapshot.error
      : null) ??
    active?.recovery_reason ??
    null;

  const filteredSessions = useMemo(() => {
    const query = search.trim().toLocaleLowerCase();
    if (!query) return sessions;
    return sessions.filter((session) =>
      session.title.toLocaleLowerCase().includes(query),
    );
  }, [search, sessions]);

  async function recover() {
    if (!active) return;
    setError(null);
    try {
      const recovered = await recoverRecorderSession(active.id);
      displaySession(recovered);
      upsertSession(recovered);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function exportWav() {
    if (!active) return;
    setError(null);
    try {
      await exportRecorderWav(active.id);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function exportTranscript() {
    if (!active) return;
    setError(null);
    try {
      await saveTextDialog(transcript, `${active.title}.txt`);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function prettify() {
    if (!active) return;
    const sessionId = active.id;
    setError(null);
    setPolishBusy(true);
    try {
      const candidate = await generateRecorderPolish(sessionId);
      if (activeId.current !== sessionId) return;
      const updated = await acceptRecorderPolish(sessionId, candidate);
      if (activeId.current === sessionId) {
        setSegmentEdits({});
        displaySession(updated);
        upsertSession(updated);
      }
    } catch (reason) {
      if (activeId.current === sessionId) setError(String(reason));
    } finally {
      setPolishBusy(false);
    }
  }

  async function restoreOriginal() {
    if (!active) return;
    const sessionId = active.id;
    setError(null);
    setPolishBusy(true);
    try {
      const updated = await revertRecorderPolish(sessionId);
      if (activeId.current === sessionId) {
        displaySession(updated);
        upsertSession(updated);
      }
    } catch (reason) {
      if (activeId.current === sessionId) setError(String(reason));
    } finally {
      setPolishBusy(false);
    }
  }

  async function clearTranscript() {
    if (!active) return;
    const sessionId = active.id;
    setError(null);
    try {
      const cleared = await clearRecorderTranscript(sessionId);
      if (activeId.current === sessionId) {
        setSegmentEdits({});
        displaySession(cleared);
      }
      upsertSession(cleared);
      setClearPending(false);
    } catch (reason) {
      setError(String(reason));
      setClearPending(false);
    }
  }

  return (
    <div className="app recorder-view">
      <header className="wp-header" data-tauri-drag-region="deep">
        <div className="wp-header-lead">
          <div className="wp-header-left">
            <span
              className="wp-traffic-space"
              aria-hidden="true"
              data-tauri-drag-region
            />
            <button
              type="button"
              className="wp-logo-button"
              aria-label="Collapse to floating bubble"
              onClick={() => void collapseToBubble()}
            >
              <AppLogo size={28} />
            </button>
            <div className="wp-action-group">
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="Toggle sidebar"
                aria-pressed={sidebarOpen}
                onClick={() => setSidebarOpen((value) => !value)}
              >
                <Icon name="panel-left" size={18} />
              </button>
              <span className="wp-sep" />
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="New recording"
                title="New recording"
                onClick={() => void newRecording()}
                disabled={ownsCapture || creatingDraft}
              >
                <Icon name="plus" size={18} />
              </button>
              <span className="wp-sep" />
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="Settings"
                onClick={onOpenSettings}
                disabled={snapshot === null}
              >
                <Icon name="settings" size={18} />
              </button>
            </div>
          </div>

          <div className="wp-title-group">
            <h1 className="wp-title">{active?.title ?? "New recording"}</h1>
            <button
              type="button"
              className="wp-icon-btn wp-icon-btn--ghost"
              aria-label="Rename recording"
              onClick={() => active && openRename(active.id, active.title)}
              disabled={!active || destructiveDisabled}
            >
              <Icon name="pencil" size={14} />
            </button>
            <button
              type="button"
              className="wp-icon-btn wp-icon-btn--ghost"
              aria-label="Delete recording"
              onClick={() =>
                active &&
                setDeleteTarget({ id: active.id, title: active.title })
              }
              disabled={!active || destructiveDisabled}
            >
              <Icon name="trash-2" size={14} />
            </button>
          </div>
        </div>

        <div className="wp-header-right">
          <div
            className={`wp-status wp-status--${widget.statusKey}`}
            role="status"
          >
            <Icon
              name={widget.icon}
              size={14}
              className={`${widget.spinning ? "wp-spin " : ""}wp-tone--${widget.tone} wp-status--${widget.statusKey}`}
            />
            <span
              className={`wp-status-label wp-tone--${widget.tone} wp-status--${widget.statusKey}`}
            >
              {widget.label}
            </span>
            {widget.showTimer && (
              <span className="wp-status-timer" aria-hidden="true">
                {formatElapsedClock(elapsed)}
              </span>
            )}
          </div>

          <div className="wp-action-group">
            <ActionIcon
              icon="play"
              label="Start"
              onClick={() => void start()}
              disabled={!canStart}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="square"
              label="Stop"
              onClick={() => void stop()}
              disabled={!canStop}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="sparkles"
              label="Prettify transcript"
              accent
              onClick={() => void prettify()}
              disabled={
                !active ||
                polishBusy ||
                hasPendingSegmentEdits ||
                destructiveDisabled ||
                !rawTranscript.trim()
              }
            />
            <span className="wp-sep" />
            <CopyButton
              text={transcript}
              resetKey={active?.id ?? null}
              onError={setError}
              onCopied={() => setError(null)}
              disabled={!transcript.trim()}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="download"
              label="Export transcript"
              onClick={() => void exportTranscript()}
              disabled={!active || !transcript.trim()}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="trash-2"
              label="Clear transcript"
              onClick={() => setClearPending(true)}
              disabled={
                !active ||
                hasPendingSegmentEdits ||
                destructiveDisabled ||
                !transcript.trim()
              }
            />
          </div>
        </div>
      </header>

      <div className="wp-info-bar">
        <div className="wp-info-left">
          <span className="wp-info-label">Audio Source:</span>
          <span className="wp-file-chip">
            <Icon name="mic" size={14} />
            Default microphone
          </span>
        </div>
        <div className="wp-info-right">
          {active && (
            <button
              type="button"
              className="wp-icon-btn"
              aria-label="Export WAV"
              title="Export WAV"
              onClick={() => void exportWav()}
              disabled={active.status !== "completed" || active.is_draft}
            >
              <Icon name="download" size={16} />
            </button>
          )}
          <span className="wp-info-meta">
            {active ? formatDuration(active.duration_ms) : "—"}
          </span>
        </div>
      </div>

      <div className="wp-main">
        {sidebarOpen && (
          <aside className="wp-sidebar">
            <ModeToggle
              mode="recorder"
              onSelectMeeting={onSelectMeeting}
              onSelectStreaming={onSelectStreaming}
              onSelectRecorder={() => {}}
            />
            <div className="wp-search">
              <Icon name="search" size={16} />
              <input
                type="search"
                className="wp-search-input"
                placeholder="Search recordings..."
                aria-label="Search recordings"
                value={search}
                onChange={(event) => setSearch(event.target.value)}
              />
            </div>

            {sessions.length === 0 ? (
              <p className="wp-info-muted">No recordings yet</p>
            ) : filteredSessions.length === 0 ? (
              <p className="wp-info-muted">No matches</p>
            ) : (
              <ul className="wp-meeting-list" role="list">
                {filteredSessions.map((session) => (
                  <StreamingSessionRow
                    key={session.id}
                    title={session.title}
                    when={new Date(session.created_at_ms).toLocaleDateString()}
                    dur={formatDuration(session.duration_ms)}
                    status={
                      snapshot?.source === "recorder" &&
                      session.id === snapshot.session_id &&
                      snapshotWidget !== null
                        ? snapshotWidget
                        : session.id === active?.id
                          ? widget
                          : persistedStatus(session.status, session.is_draft)
                    }
                    selected={session.id === active?.id}
                    onSelect={() => void openSession(session.id)}
                    onRename={() => openRename(session.id, session.title)}
                    onDelete={() =>
                      setDeleteTarget({ id: session.id, title: session.title })
                    }
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
                <h2 className="wp-transcript-title">Transcript</h2>
                {active && (
                  <span className="wp-transcript-meta">
                    {active.segments.length} segments · Editable
                  </span>
                )}
              </div>
              {active?.polished_text && (
                <button
                  type="button"
                  className="wp-icon-btn wp-icon-btn--ghost"
                  aria-label="Restore original transcript"
                  title="Restore original transcript"
                  onClick={() => void restoreOriginal()}
                  disabled={polishBusy}
                >
                  <Icon name="rotate-ccw" size={15} />
                </button>
              )}
            </div>
            <div className="wp-separator" />
            <div
              ref={transcriptScrollRef}
              className="wp-transcript-content wp-transcript-content--inset recorder-transcript"
              aria-label="Recorder transcript"
              onScroll={handleTranscriptScroll}
            >
              {effectiveError && (
                <p className="wp-error" role="alert">
                  {effectiveError}
                </p>
              )}
              {!active ? (
                <div className="wp-empty-state">
                  <Icon name="mic" size={28} />
                  <p>Start a voice recording or open a saved recording.</p>
                </div>
              ) : active.polished_text ? (
                <p className="streaming-transcript-text">
                  {active.polished_text}
                </p>
              ) : active.segments.length === 0 && !partial ? (
                <div className="wp-empty-state">
                  <Icon name="messages-square" size={28} />
                  <p>The transcript will appear here while you dictate.</p>
                </div>
              ) : (
                <>
                  <div className="streaming-transcript-text">
                    {transcriptParagraphs.map((paragraph) => (
                      <p key={paragraph[0].id} className="streaming-paragraph">
                        {paragraph.map((segment) => (
                          <span key={segment.id}>
                            <span
                              className="streaming-window recorder-segment-text"
                              role="textbox"
                              aria-label={`Transcript segment ${segment.id}`}
                              contentEditable
                              suppressContentEditableWarning
                              spellCheck
                              title={`${Math.floor(segment.start_sample / active.sample_rate)}–${Math.ceil(segment.end_sample / active.sample_rate)}s (${segment.language})`}
                              onBlur={(event) =>
                                void commitEdit(
                                  segment,
                                  event.currentTarget.textContent ?? "",
                                )
                              }
                              onInput={(event) => {
                                const text =
                                  event.currentTarget.textContent ?? "";
                                segmentEditVersions.current.set(
                                  segment.id,
                                  (segmentEditVersions.current.get(
                                    segment.id,
                                  ) ?? 0) + 1,
                                );
                                setSegmentEdits((current) => ({
                                  ...current,
                                  [segment.id]: text,
                                }));
                              }}
                            >
                              {segment.text}
                            </span>{" "}
                          </span>
                        ))}
                      </p>
                    ))}
                  </div>
                  {partial && (
                    <p className="wp-streaming-partial" role="status">
                      {partialDisplay.stable && (
                        <span className="wp-streaming-partial-stable">
                          {partialDisplay.stable}{" "}
                        </span>
                      )}
                      {partialDisplay.unstable && (
                        <em className="wp-streaming-partial-unstable">
                          {partialDisplay.unstable}
                        </em>
                      )}
                    </p>
                  )}
                </>
              )}
              {ownsCapture && (
                <div
                  className="wp-streaming-autoscroll-tail"
                  aria-hidden="true"
                />
              )}
              {active?.audio_path &&
                active.status === "completed" &&
                !active.is_draft && (
                  <audio
                    className="recorder-audio"
                    controls
                    preload="metadata"
                    src={convertFileSrc(active.audio_path)}
                  >
                    Saved Recorder audio
                  </audio>
                )}
              {active?.status === "recoverable" && (
                <button type="button" onClick={() => void recover()}>
                  Recover
                </button>
              )}
              {active?.status === "delete_failed" && (
                <button
                  type="button"
                  onClick={() =>
                    void deleteSession({ id: active.id, title: active.title })
                  }
                >
                  Retry delete
                </button>
              )}
            </div>
          </div>
        </section>
      </div>

      {renameTarget && (
        <div className="modal-overlay">
          <form
            className="modal-panel confirm-modal"
            role="dialog"
            aria-modal="true"
            aria-label="Rename recording"
            onSubmit={(event) => void saveRename(event)}
            onKeyDown={(event) => {
              if (event.key === "Escape") setRenameTarget(null);
            }}
          >
            <div className="modal-header">
              <span className="modal-title">Rename recording</span>
            </div>
            <label htmlFor="recorder-title">Recording title</label>
            <input
              id="recorder-title"
              type="text"
              aria-label="Recorder title"
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
              <button type="button" onClick={() => setRenameTarget(null)}>
                Cancel
              </button>
              <button type="submit">Save rename</button>
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
              This permanently removes the recording, its transcript, and its
              app-owned audio.
            </p>
            <div className="confirm-actions">
              <button type="button" onClick={() => setDeleteTarget(null)}>
                Cancel
              </button>
              <button
                type="button"
                onClick={() => void deleteSession(deleteTarget)}
              >
                Confirm delete
              </button>
            </div>
          </div>
        </div>
      )}

      {clearPending && active && (
        <div className="modal-overlay">
          <div
            className="modal-panel confirm-modal"
            role="alertdialog"
            aria-modal="true"
            aria-label="Clear transcript"
            onKeyDown={(event) => {
              if (event.key === "Escape") setClearPending(false);
            }}
          >
            <div className="modal-header">
              <span className="modal-title">Clear transcript?</span>
            </div>
            <p className="confirm-warning">
              Raw and prettified text will be removed. The saved audio stays
              available.
            </p>
            <div className="confirm-actions">
              <button type="button" onClick={() => setClearPending(false)}>
                Cancel
              </button>
              <button type="button" onClick={() => void clearTranscript()}>
                Clear
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
