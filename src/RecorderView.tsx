import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  acceptRecorderPolish,
  clearRecorderRecording,
  collapseToBubble,
  deleteRecorderSession,
  exportRecorderWav,
  generateRecorderPolish,
  listRecorderSessions,
  openRecorderSession,
  recoverRecorderSession,
  renameRecorderSession,
  revertRecorderPolish,
  saveTextDialog,
  stopRecorderSession,
  updateRecorderSegment,
  type LiveCaptureSnapshot,
  type RecorderSegment,
  type RecorderSession,
  type RecorderSessionSummary,
  type RecorderStatus,
} from "./ipc";
import {
  filterRecorderSessions,
  persistedRecorderStatus,
  recorderCaptureStatus,
  resolveRecorderError,
  resolveRecorderStatus,
} from "./recorderStatus";
import { RecorderDialogs, type RecorderDialogTarget } from "./RecorderDialogs";
import { RecorderInfoBar } from "./RecorderInfoBar";
import { RecorderSidebar } from "./RecorderSidebar";
import { RecorderHeader } from "./RecorderHeader";
import {
  createRecorderDraftAction,
  runRecorderAction,
  startRecorderCaptureAction,
} from "./recorderCaptureActions";
import { RecorderTranscriptPanel } from "./RecorderTranscriptPanel";
import { formatElapsedClock } from "./format";
import { type StablePartial } from "./partialStability";
import { useRecorderEventListeners } from "./useRecorderEventListeners";
import { groupWindowsIntoParagraphs } from "./paragraphs";
const AUTOSCROLL_RESUME_THRESHOLD_PX = 48;

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
  const [sessionsHydrated, setSessionsHydrated] = useState(false);
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
  const [renameTarget, setRenameTarget] = useState<RecorderDialogTarget | null>(
    null,
  );
  const [renameDraft, setRenameDraft] = useState("");
  const [renameError, setRenameError] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<RecorderDialogTarget | null>(
    null,
  );
  const [clearPending, setClearPending] = useState(false);
  const [polishingId, setPolishingId] = useState<number | null>(null);
  const [creatingDraft, setCreatingDraft] = useState(false);
  const [startPending, setStartPending] = useState(false);
  const [segmentEdits, setSegmentEdits] = useState<Record<number, string>>({});
  const [editingSegmentId, setEditingSegmentId] = useState<number | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const partialCursor = useRef<{ sessionId: number | null; revision: number }>({
    sessionId: null,
    revision: -1,
  });
  const activeId = useRef<number | null>(null);
  const libraryRevision = useRef(0);
  const initialSelectionAttempted = useRef(false);
  const openRequest = useRef(0);
  // A delete may finish while an earlier `openRecorderSession` request is
  // still resolving. Keep the invalidation scoped to that id so a later open
  // for a different row remains valid.
  const deletedSessionIds = useRef(new Set<number>());
  const openGenerations = useRef(new Map<number, number>());
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
    if (deletedSessionIds.current.has(session.id)) return;
    libraryRevision.current += 1;
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
        setEditingSegmentId(null);
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
    const hydrationRevision = libraryRevision.current;
    void listRecorderSessions()
      .then((items) => {
        // A slow initial list must not replace an item created, renamed,
        // deleted, or received through an event while that list was in flight.
        if (!cancelled && libraryRevision.current === hydrationRevision) {
          setSessions(items);
        }
      })
      .catch((reason) => {
        if (!cancelled) setError(String(reason));
      })
      .finally(() => {
        if (!cancelled) setSessionsHydrated(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useRecorderEventListeners({
    activeId,
    activeStatus,
    openRequest,
    deletedSessionIds,
    previousPartial,
    partialCursor,
    setSnapshot,
    upsertSession,
    displaySession,
    setActive,
    resetPartial,
    setPartialDisplay,
    setPartial,
    setError,
  });

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
    const request = ++openRequest.current;
    const generation = (openGenerations.current.get(sessionId) ?? 0) + 1;
    openGenerations.current.set(sessionId, generation);
    void openRecorderSession(sessionId)
      .then((session) => {
        if (
          !cancelled &&
          request === openRequest.current &&
          openGenerations.current.get(sessionId) === generation &&
          !deletedSessionIds.current.has(sessionId) &&
          session.id === sessionId
        ) {
          displaySession(session);
          upsertSession(session);
        }
      })
      .catch((reason) => {
        if (
          !cancelled &&
          request === openRequest.current &&
          openGenerations.current.get(sessionId) === generation &&
          !deletedSessionIds.current.has(sessionId)
        ) {
          setError(String(reason));
        }
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
  const selectedSessionCanStart = !active || active.status === "completed";
  const canStart =
    selectedSessionCanStart &&
    !captureBusy &&
    !startPending &&
    !recorderStartPending;
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

  const openSession = useCallback(
    async (id: number) => {
      initialSelectionAttempted.current = true;
      const request = ++openRequest.current;
      const generation = (openGenerations.current.get(id) ?? 0) + 1;
      openGenerations.current.set(id, generation);
      setError(null);
      try {
        const session = await openRecorderSession(id);
        if (
          request === openRequest.current &&
          openGenerations.current.get(id) === generation &&
          !deletedSessionIds.current.has(id) &&
          session.id === id
        ) {
          displaySession(session);
        }
      } catch (reason) {
        if (
          request === openRequest.current &&
          openGenerations.current.get(id) === generation &&
          !deletedSessionIds.current.has(id)
        ) {
          setError(String(reason));
        }
      }
    },
    [displaySession],
  );

  useEffect(() => {
    if (
      !sessionsHydrated ||
      snapshot === null ||
      initialSelectionAttempted.current
    ) {
      return;
    }
    initialSelectionAttempted.current = true;
    if (ownsCapture || activeId.current !== null) return;
    const first = sessions[0];
    if (first) void openSession(first.id);
  }, [openSession, ownsCapture, sessions, sessionsHydrated, snapshot]);

  async function newRecording() {
    if (ownsCapture || creatingDraft) return;
    setError(null);
    setCreatingDraft(true);
    await createRecorderDraftAction({
      onCreated: (draft) => {
        openRequest.current += 1;
        deletedSessionIds.current.delete(draft.id);
        autoScrollEnabledRef.current = true;
        displaySession(draft);
        upsertSession(draft);
      },
      onError: (error) => setError(String(error)),
    });
    setCreatingDraft(false);
  }

  async function start() {
    if (startPendingRef.current || !selectedSessionCanStart) return;
    startPendingRef.current = true;
    setStartPending(true);
    onRecorderStartPendingChange(true);
    setError(null);
    await startRecorderCaptureAction({
      sessionId: active?.id,
      onStarted: (session) => {
        displaySession(session);
        upsertSession(session);
      },
      onError: (error) => setError(String(error)),
    });
    startPendingRef.current = false;
    setStartPending(false);
    onRecorderStartPendingChange(false);
  }

  const stop = () => runRecorderAction(stopRecorderSession, setError);

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

  function openDelete(target: { id: number; title: string }) {
    setDeleteTarget(target);
  }

  function closeDelete() {
    setDeleteTarget(null);
  }

  async function deleteSession(target: RecorderDialogTarget) {
    const id = target.id;
    // Guard the in-flight initial list before the destructive IPC awaits.
    // Otherwise an old list response can resurrect an item just deleted here.
    libraryRevision.current += 1;
    setError(null);
    try {
      await deleteRecorderSession(id);
      deletedSessionIds.current.add(id);
      openGenerations.current.set(
        id,
        (openGenerations.current.get(id) ?? 0) + 1,
      );
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
      closeDelete();
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
  const activePolishing = polishingId === active?.id;
  // The model is process-wide single-flight even though the visible status is
  // owned by the recording which started the operation.
  const polishBusy = polishingId !== null;
  const widget = resolveRecorderStatus(active, snapshot, activePolishing);
  const snapshotWidget = recorderCaptureStatus(snapshot);
  const effectiveError = resolveRecorderError(error, snapshot, active);

  const filteredSessions = useMemo(
    () => filterRecorderSessions(sessions, search),
    [search, sessions],
  );

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

  const exportWav = () =>
    active && runRecorderAction(() => exportRecorderWav(active.id), setError);

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
    setPolishingId(sessionId);
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
      setPolishingId((current) => (current === sessionId ? null : current));
    }
  }

  async function restoreOriginal() {
    if (!active) return;
    const sessionId = active.id;
    setError(null);
    setPolishingId(sessionId);
    try {
      const updated = await revertRecorderPolish(sessionId);
      if (activeId.current === sessionId) {
        displaySession(updated);
        upsertSession(updated);
      }
    } catch (reason) {
      if (activeId.current === sessionId) setError(String(reason));
    } finally {
      setPolishingId((current) => (current === sessionId ? null : current));
    }
  }

  async function clearRecording() {
    if (!active) return;
    const sessionId = active.id;
    setError(null);
    try {
      const cleared = await clearRecorderRecording(sessionId);
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
      <RecorderHeader
        active={active}
        sidebarOpen={sidebarOpen}
        captureReady={snapshot !== null}
        newRecordingDisabled={ownsCapture || creatingDraft}
        destructiveDisabled={destructiveDisabled}
        widget={widget}
        elapsedLabel={formatElapsedClock(elapsed)}
        transcript={transcript}
        rawTranscript={rawTranscript}
        canStart={canStart}
        canStop={canStop}
        polishBusy={polishBusy}
        hasPendingSegmentEdits={hasPendingSegmentEdits}
        onCollapse={() => void collapseToBubble()}
        onToggleSidebar={() => setSidebarOpen((value) => !value)}
        onNewRecording={() => void newRecording()}
        onOpenSettings={onOpenSettings}
        onRename={openRename}
        onDelete={(id, title) => openDelete({ id, title })}
        onStart={() => void start()}
        onStop={() => void stop()}
        onPrettify={() => void prettify()}
        onExport={() => void exportTranscript()}
        onClear={() => setClearPending(true)}
        onError={setError}
      />

      <RecorderInfoBar active={active} onExportWav={() => void exportWav()} />

      <div className="wp-main">
        {sidebarOpen && (
          <RecorderSidebar
            sessions={sessions}
            filteredSessions={filteredSessions}
            activeId={active?.id ?? null}
            search={search}
            onSearchChange={setSearch}
            onSelectMeeting={onSelectMeeting}
            onSelectStreaming={onSelectStreaming}
            onOpenSession={(id) => void openSession(id)}
            onRename={openRename}
            onDelete={(id, title) => openDelete({ id, title })}
            statusFor={(session) =>
              snapshot?.source === "recorder" &&
              session.id === snapshot.session_id &&
              snapshotWidget !== null
                ? snapshotWidget
                : session.id === active?.id
                  ? widget
                  : persistedRecorderStatus(session.status, session.is_draft)
            }
          />
        )}

        <RecorderTranscriptPanel
          active={active}
          transcriptParagraphs={transcriptParagraphs}
          partial={partial}
          partialDisplay={partialDisplay}
          effectiveError={effectiveError}
          ownsCapture={ownsCapture}
          polishBusy={polishBusy}
          editingSegmentId={editingSegmentId}
          transcriptScrollRef={transcriptScrollRef}
          onScroll={handleTranscriptScroll}
          onRestoreOriginal={() => void restoreOriginal()}
          onEditingChange={setEditingSegmentId}
          onCommitEdit={(segment, text) => void commitEdit(segment, text)}
          onEditChange={(segmentId, text) => {
            segmentEditVersions.current.set(
              segmentId,
              (segmentEditVersions.current.get(segmentId) ?? 0) + 1,
            );
            setSegmentEdits((current) => ({ ...current, [segmentId]: text }));
          }}
          onRecover={() => void recover()}
          onRetryDelete={() =>
            active && void deleteSession({ id: active.id, title: active.title })
          }
        />
      </div>

      <RecorderDialogs
        renameTarget={renameTarget}
        renameDraft={renameDraft}
        renameError={renameError}
        onRenameDraftChange={(value) => {
          setRenameDraft(value);
          setRenameError(null);
        }}
        onRenameCancel={() => setRenameTarget(null)}
        onSaveRename={(event) => void saveRename(event)}
        deleteTarget={deleteTarget}
        onDeleteCancel={closeDelete}
        onDelete={(target) => void deleteSession(target)}
        clearPending={clearPending}
        active={active}
        onClearCancel={() => setClearPending(false)}
        onClear={() => void clearRecording()}
      />
    </div>
  );
}
