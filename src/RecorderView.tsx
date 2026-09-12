import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  acceptRecorderPolish,
  collapseToBubble,
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
  revertRecorderPolish,
  renameRecorderSession,
  requestMicrophonePermission,
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

const STATUS_LABEL: Record<RecorderStatus, string> = {
  recording: "Recording",
  finalizing: "Finalizing",
  completed: "Completed",
  recoverable: "Recoverable",
  delete_failed: "Delete failed",
};

interface RecorderViewProps {
  onSelectMeeting: () => void;
  onSelectStreaming: () => void;
  onOpenSettings: () => void;
  meetingTranscriptionActive: boolean;
}

export function RecorderView({
  onSelectMeeting,
  onSelectStreaming,
  onOpenSettings,
  meetingTranscriptionActive,
}: RecorderViewProps) {
  const [sessions, setSessions] = useState<RecorderSessionSummary[]>([]);
  const [active, setActive] = useState<RecorderSession | null>(null);
  const [snapshot, setSnapshot] = useState<LiveCaptureSnapshot | null>(null);
  const [partial, setPartial] = useState("");
  const partialCursor = useRef<{ sessionId: number | null; revision: number }>({
    sessionId: null,
    revision: -1,
  });
  const activeId = useRef<number | null>(null);
  const activeStatus = useRef<RecorderStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState<string | null>(null);
  const [deletePending, setDeletePending] = useState(false);
  const [polishCandidate, setPolishCandidate] = useState<{
    sessionId: number;
    text: string;
  } | null>(null);
  const [polishBusy, setPolishBusy] = useState(false);

  const upsertSession = useCallback((session: RecorderSessionSummary) => {
    setSessions((current) => [
      session,
      ...current.filter((item) => item.id !== session.id),
    ]);
  }, []);

  const displaySession = useCallback((session: RecorderSession) => {
    if (activeId.current !== session.id || session.status !== "recording") {
      setPartial("");
      partialCursor.current = { sessionId: session.id, revision: -1 };
    }
    activeId.current = session.id;
    activeStatus.current = session.status;
    setActive(session);
    setPolishCandidate(null);
  }, []);

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
          if (!cancelled)
            setSnapshot((current) =>
              reconcileLiveCaptureSnapshot(current, next),
            );
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
          if (cancelled) return;
          if (activeId.current !== segment.session_id) return;
          setActive((current) => {
            if (!current || current.id !== segment.session_id) return current;
            const withoutSame = current.segments.filter(
              (item) => item.id !== segment.id,
            );
            return { ...current, segments: [...withoutSame, segment] };
          });
          setPartial("");
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
          setPartial(next.text);
        }),
        await onRecorderError((next) => {
          if (!cancelled) setError(next.message);
        }),
      );
      const initial = await getLiveCaptureSnapshot();
      if (!cancelled)
        setSnapshot((current) =>
          reconcileLiveCaptureSnapshot(current, initial),
        );
    };
    void register();
    return () => {
      cancelled = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [displaySession, upsertSession]);

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
  const canStart = !captureBusy;
  const canStop = ownsCapture && snapshot?.phase === "capturing";

  async function openSession(id: number) {
    setError(null);
    try {
      const session = await openRecorderSession(id);
      displaySession(session);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function start() {
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
      const session = await startRecorderSession();
      displaySession(session);
      upsertSession(session);
    } catch (reason) {
      setError(String(reason));
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

  async function saveRename() {
    if (!active || renameDraft === null) return;
    try {
      const renamed = await renameRecorderSession(
        active.id,
        renameDraft.trim(),
      );
      displaySession(renamed);
      upsertSession(renamed);
      setRenameDraft(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function confirmDelete() {
    if (!active) return;
    const id = active.id;
    try {
      await deleteRecorderSession(id);
      setSessions((items) => items.filter((item) => item.id !== id));
      activeId.current = null;
      activeStatus.current = null;
      setActive(null);
      setDeletePending(false);
    } catch (reason) {
      setError(String(reason));
      setDeletePending(false);
      try {
        const refreshed = await openRecorderSession(id);
        displaySession(refreshed);
        upsertSession(refreshed);
      } catch {
        // The original actionable failure remains visible.
      }
    }
  }

  async function commitEdit(segment: RecorderSegment, text: string) {
    if (!active) return;
    try {
      const updated = await updateRecorderSegment(active.id, segment.id, text);
      setActive((current) =>
        current && current.id === active.id
          ? {
              ...current,
              segments: current.segments.map((item) =>
                item.id === segment.id ? updated : item,
              ),
            }
          : current,
      );
    } catch (reason) {
      setError(String(reason));
    }
  }

  const rawTranscript = useMemo(
    () => active?.segments.map((segment) => segment.text).join("\n") ?? "",
    [active],
  );
  const transcript = active?.polished_text ?? rawTranscript;
  const asrLabel =
    active?.asr_model_id === "qwen3-asr-0.6b"
      ? "Qwen3-ASR 0.6B"
      : "Whisper large-v3-turbo";
  const asrLanguage =
    active?.asr_language === "ru"
      ? "Russian"
      : active?.asr_language === "en"
        ? "English"
        : "Auto / mixed";
  const destructiveDisabled =
    active?.status === "recording" ||
    active?.status === "finalizing" ||
    (ownsCapture && active?.id === snapshot?.session_id);
  const lifecycleLabel = (() => {
    if (snapshot?.source === "recorder") {
      if (snapshot.phase === "starting") return "Starting";
      if (snapshot.phase === "capturing") return "Recording";
      if (snapshot.phase === "stopping") return "Stopping · Finalizing";
      if (snapshot.phase === "error") return "Error";
    }
    return active ? STATUS_LABEL[active.status] : snapshot ? "Idle" : "Loading";
  })();
  const lifecycleClass =
    snapshot?.source === "recorder" && snapshot.phase !== "idle"
      ? snapshot.phase
      : (active?.status ?? "idle");
  const effectiveError =
    error ??
    (snapshot?.source === "recorder" && snapshot.phase === "error"
      ? snapshot.error
      : null) ??
    active?.recovery_reason ??
    null;

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

  async function generatePolish() {
    if (!active) return;
    const sessionId = active.id;
    setError(null);
    setPolishBusy(true);
    try {
      const text = await generateRecorderPolish(sessionId);
      if (activeId.current === sessionId) {
        setPolishCandidate({ sessionId, text });
      }
    } catch (reason) {
      if (activeId.current === sessionId) setError(String(reason));
    } finally {
      setPolishBusy(false);
    }
  }

  async function acceptPolish() {
    if (
      !active ||
      polishCandidate === null ||
      polishCandidate.sessionId !== active.id
    ) {
      return;
    }
    const { sessionId, text } = polishCandidate;
    setError(null);
    setPolishBusy(true);
    try {
      const updated = await acceptRecorderPolish(sessionId, text);
      upsertSession(updated);
      if (activeId.current === sessionId) displaySession(updated);
    } catch (reason) {
      if (activeId.current === sessionId) setError(String(reason));
    } finally {
      setPolishBusy(false);
    }
  }

  async function revertPolish() {
    if (!active) return;
    const sessionId = active.id;
    setError(null);
    setPolishBusy(true);
    try {
      const updated = await revertRecorderPolish(sessionId);
      upsertSession(updated);
      if (activeId.current === sessionId) displaySession(updated);
    } catch (reason) {
      if (activeId.current === sessionId) setError(String(reason));
    } finally {
      setPolishBusy(false);
    }
  }

  return (
    <div className="app recorder-view">
      <header className="wp-header" data-tauri-drag-region="deep">
        <div className="wp-header-lead">
          <span className="wp-traffic-space" aria-hidden="true" />
          <button
            type="button"
            className="wp-logo-button"
            aria-label="Collapse to floating bubble"
            onClick={() => void collapseToBubble()}
          >
            <AppLogo size={28} />
          </button>
          <strong>{active?.title ?? "Recorder"}</strong>
        </div>
        <div className="wp-header-actions">
          {ownsCapture ? (
            <button
              type="button"
              className="wp-btn wp-btn--danger"
              onClick={stop}
              disabled={!canStop}
            >
              {snapshot?.phase === "starting"
                ? "Starting…"
                : snapshot?.phase === "stopping"
                  ? "Stopping…"
                  : "Stop"}
            </button>
          ) : (
            <button
              type="button"
              className="wp-btn wp-btn--primary"
              onClick={start}
              disabled={!canStart}
            >
              Start
            </button>
          )}
          <button
            type="button"
            className="wp-icon-btn"
            aria-label="Settings"
            onClick={onOpenSettings}
          >
            <Icon name="settings" size={18} />
          </button>
        </div>
      </header>

      <div className="wp-main">
        <aside className="wp-sidebar">
          <ModeToggle
            mode="recorder"
            onSelectMeeting={onSelectMeeting}
            onSelectStreaming={onSelectStreaming}
            onSelectRecorder={() => {}}
          />
          <ul className="wp-meeting-list" role="list">
            {sessions.map((session) => (
              <li key={session.id}>
                <button
                  type="button"
                  className="wp-session-row"
                  aria-label={`Open ${session.title}`}
                  onClick={() => void openSession(session.id)}
                >
                  <span>{session.title}</span>
                  <small>{STATUS_LABEL[session.status]}</small>
                </button>
              </li>
            ))}
          </ul>
        </aside>

        <main className="wp-content recorder-content">
          <span
            role="status"
            className={`recorder-status recorder-status--${lifecycleClass}`}
          >
            {lifecycleLabel}
          </span>
          {active ? (
            <>
              <p className="settings-description">
                {asrLabel} · {asrLanguage} · local {active.sample_rate / 1_000}
                kHz audio
              </p>
              <div className="recorder-toolbar">
                <button
                  type="button"
                  onClick={() => setRenameDraft(active.title)}
                  aria-label={`Rename ${active.title}`}
                >
                  Rename
                </button>
                <button
                  type="button"
                  onClick={() => setDeletePending(true)}
                  aria-label={`Delete ${active.title}`}
                  disabled={destructiveDisabled}
                >
                  Delete
                </button>
                <button
                  type="button"
                  onClick={() =>
                    void navigator.clipboard
                      .writeText(transcript)
                      .catch((reason) => setError(String(reason)))
                  }
                >
                  Copy transcript
                </button>
                <button
                  type="button"
                  onClick={() => void exportWav()}
                  disabled={active.status !== "completed"}
                >
                  Export WAV
                </button>
                <button type="button" onClick={() => void exportTranscript()}>
                  Export transcript
                </button>
                {active.polished_text ? (
                  <button
                    type="button"
                    onClick={() => void revertPolish()}
                    disabled={polishBusy}
                  >
                    Revert polish
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => void generatePolish()}
                    disabled={
                      polishBusy || destructiveDisabled || !rawTranscript.trim()
                    }
                  >
                    {polishBusy ? "Polishing…" : "Polish transcript"}
                  </button>
                )}
              </div>
              {renameDraft !== null && (
                <div className="recorder-inline-dialog">
                  <input
                    aria-label="Recorder title"
                    value={renameDraft}
                    onChange={(event) => setRenameDraft(event.target.value)}
                  />
                  <button type="button" onClick={() => void saveRename()}>
                    Save rename
                  </button>
                </div>
              )}
              {deletePending && (
                <div
                  className="recorder-inline-dialog"
                  role="alertdialog"
                  aria-label="Delete recording"
                >
                  <span>Delete this recording and its app-owned audio?</span>
                  <button type="button" onClick={() => void confirmDelete()}>
                    Confirm delete
                  </button>
                </div>
              )}
              {polishCandidate !== null && (
                <section
                  className="recorder-polish-review"
                  aria-label="Polish review"
                >
                  <strong>Polished candidate</strong>
                  <p>{polishCandidate.text}</p>
                  <div className="recorder-toolbar">
                    <button type="button" onClick={() => void acceptPolish()}>
                      Accept polish
                    </button>
                    <button
                      type="button"
                      onClick={() => setPolishCandidate(null)}
                    >
                      Cancel polish
                    </button>
                  </div>
                </section>
              )}
              {active.polished_text ? (
                <div
                  className="recorder-polished"
                  aria-label="Polished transcript"
                >
                  {active.polished_text}
                </div>
              ) : (
                <div
                  className="recorder-transcript"
                  aria-label="Recorder transcript"
                >
                  {active.segments.map((segment) => (
                    <textarea
                      key={segment.id}
                      defaultValue={segment.text}
                      aria-label={`Transcript segment ${segment.id}`}
                      onBlur={(event) =>
                        void commitEdit(segment, event.currentTarget.value)
                      }
                    />
                  ))}
                  {partial && <em className="recorder-partial">{partial}</em>}
                </div>
              )}
              {active.audio_path && active.status === "completed" && (
                <audio
                  className="recorder-audio"
                  controls
                  preload="metadata"
                  src={convertFileSrc(active.audio_path)}
                >
                  Saved Recorder audio
                </audio>
              )}
              {active.status === "recoverable" && (
                <button type="button" onClick={() => void recover()}>
                  Recover
                </button>
              )}
              {active.status === "delete_failed" && (
                <button type="button" onClick={() => void confirmDelete()}>
                  Retry delete
                </button>
              )}
            </>
          ) : (
            <div className="wp-empty-state">
              <Icon name="mic" size={28} />
              <p>Start a voice recording or open a saved note.</p>
            </div>
          )}
          {effectiveError && (
            <p className="wp-error" role="alert">
              {effectiveError}
            </p>
          )}
        </main>
      </div>
    </div>
  );
}
