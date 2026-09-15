import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  clearMeeting,
  createMeeting,
  deleteMeeting,
  generateMfu,
  getLiveCaptureSnapshot,
  getSettings,
  setSetting,
  listMeetings,
  listTaskModels,
  openFileDialog,
  setMeetingSource,
  transcribeMeeting,
  diarizeMeeting,
  onLiveCaptureState,
  onOpenRecorderWorkspace,
  saveTextDialog,
  renameMeeting,
  type Meeting as PersistedMeeting,
  type MeetingSummary,
  type MeetingMfu,
  type Segment,
  type LiveCaptureSnapshot,
} from "./ipc";
import {
  isLiveCaptureActive,
  reconcileLiveCaptureSnapshot,
} from "./liveCaptureState";
import { AppModeView } from "./AppModeView";
import { applyTheme, type Theme } from "./theme";
import { applyStatusColors, parseStatusColors } from "./statusColors";
import { formatDuration } from "./format";
import { MeetingDialogs } from "./MeetingDialogs";
import { TranscriptionWorkspace } from "./TranscriptionWorkspace";
import { speakerLabel } from "./speakerColors";
import { toSummary } from "./meetingUtils";
import { resolveMeetingStatus, type MeetingStatusView } from "./meetingStatus";
import { useMeetingAutosave } from "./useMeetingAutosave";
import { useMeetingSelection } from "./useMeetingSelection";
import { useTranscriptionEvents } from "./useTranscriptionEvents";
import {
  renderForExport,
  exportFileExtension,
  type ExportFileType,
} from "./export";
export function App() {
  const [status, setStatus] = useState<
    { kind: "idle" } | { kind: "error"; message: string }
  >({ kind: "idle" });
  const [fileName, setFileName] = useState<string | null>(null);
  const [segments, setSegments] = useState<Segment[]>([]);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isStreamingOpen, setIsStreamingOpen] = useState(false);
  const [isRecorderOpen, setIsRecorderOpen] = useState(false);
  const [recorderStartPending, setRecorderStartPending] = useState(false);
  const [liveCaptureSnapshot, setLiveCaptureSnapshot] =
    useState<LiveCaptureSnapshot | null>(null);
  const isStreamingActive =
    liveCaptureSnapshot === null || isLiveCaptureActive(liveCaptureSnapshot);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [elapsed, setElapsed] = useState(0);
  const [transcriptionModelReady, setTranscriptionModelReady] = useState<
    boolean | null
  >(null);
  const [llmModelReady, setLlmModelReady] = useState<boolean | null>(null);
  const [mfu, setMfu] = useState<MeetingMfu | null>(null);
  const [speakerLabels, setSpeakerLabels] = useState<Record<number, string>>(
    {},
  );
  const [meetingSummaries, setMeetingSummaries] = useState<MeetingSummary[]>(
    [],
  );
  const [meetingSearch, setMeetingSearch] = useState("");
  const [activeMeeting, setActiveMeeting] = useState<PersistedMeeting | null>(
    null,
  );
  const [editingSegmentIndex, setEditingSegmentIndex] = useState<number | null>(
    null,
  );
  const filteredMeetingSummaries = useMemo(() => {
    const query = meetingSearch.trim().toLocaleLowerCase();
    if (query.length < 3) return meetingSummaries;
    return meetingSummaries.filter((meeting) =>
      meeting.title.toLocaleLowerCase().includes(query),
    );
  }, [meetingSearch, meetingSummaries]);
  const [renameTarget, setRenameTarget] = useState<Pick<
    PersistedMeeting,
    "id" | "title"
  > | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [renameError, setRenameError] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<MeetingSummary | null>(null);
  const [clearPending, setClearPending] = useState(false);
  const [diarizationWarning, setDiarizationWarning] = useState<string | null>(
    null,
  );
  const [exportFileType, setExportFileType] =
    useState<ExportFileType>("plain_text");
  const [mfuPanelVisible, setMfuPanelVisible] = useState(true);
  const [transcribingId, setTranscribingId] = useState<number | null>(null);
  const [transcribingPhase, setTranscribingPhase] = useState<
    "transcribing" | "diarizing"
  >("transcribing");
  const [transcribingProgress, setTranscribingProgress] = useState<
    number | null
  >(null);
  const [generatingMfuId, setGeneratingMfuId] = useState<number | null>(null);
  const isGeneratingMfu = generatingMfuId !== null;
  const [diarizingId, setDiarizingId] = useState<number | null>(null);
  const isDiarizing = diarizingId !== null;
  const [diarizationModelReady, setDiarizationModelReady] = useState<
    boolean | null
  >(null);
  const libraryRevisionRef = useRef(0);
  const {
    activeMeetingIdRef,
    meetingOpenGenerationRef,
    applyActiveMeeting,
    upsertSummary,
    openSelectedMeeting,
    clearActiveMeeting,
  } = useMeetingSelection({
    setActiveMeeting,
    setFileName,
    setSegments,
    setEditingSegmentIndex,
    setSpeakerLabels,
    setStatus,
    setMfu,
    setMeetingSummaries,
  });
  useTranscriptionEvents({
    transcribingId,
    activeMeetingIdRef,
    setTranscribingPhase,
    setTranscribingProgress,
    setActiveMeeting,
    setFileName,
    setSegments,
    setSpeakerLabels,
    setMfu,
    upsertSummary,
  });
  const { editSegment, editMfuField, flushMeetingWrites } = useMeetingAutosave({
    activeMeetingId: activeMeeting?.id,
    activeMeetingIdRef,
    mfu,
    setMfu,
    setSegments,
    onError: (message) => setStatus({ kind: "error", message }),
  });
  // Subscribe before reading the snapshot. Revision reconciliation closes the
  // event-between-listen-and-query race and keeps Settings locked even when
  // StreamingView is unmounted.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    const applySnapshot = (incoming: LiveCaptureSnapshot) => {
      if (!cancelled) {
        setLiveCaptureSnapshot((current) =>
          reconcileLiveCaptureSnapshot(current, incoming),
        );
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
        applySnapshot(await getLiveCaptureSnapshot());
      } catch {
        // A failed lifecycle query does not manufacture an idle state. The
        // native coordinator remains authoritative and can recover on event.
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void onOpenRecorderWorkspace(() => {
      setIsSettingsOpen(false);
      setIsStreamingOpen(false);
      setIsRecorderOpen(true);
    })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (
      transcribingId === null &&
      generatingMfuId === null &&
      diarizingId === null
    )
      return;
    setElapsed(0);
    const id = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(id);
  }, [transcribingId, generatingMfuId, diarizingId]);

  const refreshModelAvailability = useCallback(async () => {
    try {
      const models = await listTaskModels();
      const settings = await getSettings();
      const transcriptionId =
        settings.active_model_transcription ?? "transcription";
      const transcription = models.find((m) => m.id === transcriptionId);
      setTranscriptionModelReady(transcription?.downloaded ?? false);
      const llmId = settings.active_model_llm;
      if (llmId) {
        const llm = models.find((m) => m.id === llmId);
        setLlmModelReady(llm?.downloaded ?? false);
      } else {
        setLlmModelReady(false);
      }
      const diarizationVariant = settings.active_model_diarization;
      if (diarizationVariant && diarizationVariant !== "none") {
        const diarization = models.find(
          (m) => m.id === `diarization-${diarizationVariant}`,
        );
        setDiarizationModelReady(diarization?.downloaded ?? false);
      } else {
        setDiarizationModelReady(false);
      }
    } catch {
      setTranscriptionModelReady(false);
      setLlmModelReady(false);
      setDiarizationModelReady(false);
    }
  }, []);

  useEffect(() => {
    void refreshModelAvailability();
  }, [refreshModelAvailability]);

  const refreshExportFileType = useCallback(async () => {
    try {
      const settings = await getSettings();
      setExportFileType(settings.export_file_type as ExportFileType);
    } catch {
      // Keep the previous selection; the export actions still work with it.
    }
  }, []);

  useEffect(() => {
    getSettings()
      .then((s) => {
        applyTheme(s.theme as Theme);
        applyStatusColors(parseStatusColors(s.status_colors));
        setMfuPanelVisible(s.mfu_panel_meeting ?? true);
      })
      .catch(() => setMfuPanelVisible(true));
    void refreshExportFileType();
  }, [refreshExportFileType]);

  const handleToggleMfuPanel = useCallback((next: boolean) => {
    setMfuPanelVisible(next);
    void (async () => {
      try {
        await setSetting("mfu_panel_meeting", next ? "true" : "false");
      } catch {
        // Best-effort persistence: the switch already reflects `next`.
      }
    })();
  }, []);

  useEffect(() => {
    let cancelled = false;

    async function loadMeetingLibrary() {
      const revision = libraryRevisionRef.current;
      try {
        const summaries = await listMeetings();
        if (cancelled || revision !== libraryRevisionRef.current) return;
        // The workspace is always backed by a real, persisted meeting. When the
        // library is empty we seed one so the initial meeting can be renamed or
        // deleted and never loses its transcript when another meeting is opened.
        if (summaries.length === 0) {
          const meeting = await createMeeting();
          if (cancelled || revision !== libraryRevisionRef.current) return;
          setMeetingSummaries([toSummary(meeting)]);
          applyActiveMeeting(meeting);
          return;
        }
        setMeetingSummaries(summaries);
        await openSelectedMeeting(summaries[0].id);
      } catch (error) {
        if (!cancelled) setStatus({ kind: "error", message: String(error) });
      }
    }

    void loadMeetingLibrary();
    return () => {
      cancelled = true;
      meetingOpenGenerationRef.current += 1;
    };
  }, [applyActiveMeeting, meetingOpenGenerationRef, openSelectedMeeting]);
  function resolveSpeakerLabel(speakerId: number): string {
    return speakerLabels[speakerId] ?? speakerLabel(speakerId);
  }

  function renameSpeaker(speakerId: number, newLabel: string) {
    setSpeakerLabels((prev) => ({ ...prev, [speakerId]: newLabel }));
  }

  // The single rendering both export-to-file and the header copy button use,
  // so the two can never drift apart (WP-15).
  const exportText = useMemo(
    () => renderForExport(exportFileType, segments, mfu, resolveSpeakerLabel),
    [exportFileType, segments, mfu, speakerLabels],
  );

  const durationLabel = useMemo(() => {
    if (segments.length === 0) return "—";
    return formatDuration(segments[segments.length - 1].end_ms);
  }, [segments]);

  // Selecting a file only attaches it to the active meeting; transcription is
  // a separate, explicit action (the Transcribe button).
  async function handleChooseFile() {
    if (!activeMeeting || isDiarizing) return;
    try {
      const path = await openFileDialog();
      if (!path) return;
      const meeting = await setMeetingSource(activeMeeting.id, path);
      applyActiveMeeting(meeting);
      upsertSummary(meeting);
    } catch (e) {
      setStatus({ kind: "error", message: String(e) });
    }
  }

  async function handleTranscribe() {
    if (isStreamingActive || !activeMeeting?.source_path) return;
    const id = activeMeeting.id;
    try {
      setTranscribingId(id);
      setTranscribingPhase("transcribing");
      setTranscribingProgress(0);
      setStatus({ kind: "idle" });
      setSegments([]);
      setSpeakerLabels({});
      const { meeting, diarization_warning } = await transcribeMeeting(id);
      upsertSummary(meeting);
      // The user may have opened another meeting while this ran; the result
      // only takes over the workspace if its meeting is still the one on
      // screen. Either way the sidebar summary above is refreshed.
      if (activeMeetingIdRef.current === meeting.id) {
        applyActiveMeeting(meeting);
        if (diarization_warning) setDiarizationWarning(diarization_warning);
      }
    } catch (e) {
      // The same rule on the way out: a failure belongs to the meeting that
      // was transcribing. Reporting it against whatever the user has opened
      // since would blame a meeting that never ran.
      if (activeMeetingIdRef.current === id)
        setStatus({ kind: "error", message: String(e) });
    } finally {
      setTranscribingProgress(null);
      setTranscribingId(null);
    }
  }

  async function handleGenerateMfu() {
    if (!activeMeeting) return;
    const id = activeMeeting.id;
    // A hidden panel is auto-revealed so the generated result is never left
    // behind it — the generation itself is unaffected either way (DoD 4).
    handleToggleMfuPanel(true);
    try {
      setGeneratingMfuId(id);
      setStatus({ kind: "idle" });
      const meeting = await generateMfu(id);
      upsertSummary(meeting);
      if (activeMeetingIdRef.current === meeting.id) {
        setMfu(meeting.mfu ?? null);
      }
    } catch (e) {
      if (activeMeetingIdRef.current === id)
        setStatus({ kind: "error", message: String(e) });
    } finally {
      setGeneratingMfuId(null);
    }
  }

  // Re-runs speaker identification alone on the active meeting's existing
  // transcript — the header "Diarize" action, separate from the diarization
  // pass folded into Transcribe.
  async function handleDiarize() {
    if (!activeMeeting) return;
    const id = activeMeeting.id;
    try {
      setDiarizingId(id);
      setStatus({ kind: "idle" });
      const { meeting, diarization_warning } = await diarizeMeeting(id);
      upsertSummary(meeting);
      if (activeMeetingIdRef.current === meeting.id) {
        applyActiveMeeting(meeting);
        if (diarization_warning) setDiarizationWarning(diarization_warning);
      }
    } catch (e) {
      if (activeMeetingIdRef.current === id)
        setStatus({ kind: "error", message: String(e) });
    } finally {
      setDiarizingId(null);
    }
  }

  async function handleRemoveFile() {
    if (!activeMeeting || isDiarizing) return;
    try {
      const meeting = await setMeetingSource(activeMeeting.id, null);
      applyActiveMeeting(meeting);
      upsertSummary(meeting);
    } catch (e) {
      setStatus({ kind: "error", message: String(e) });
    }
  }

  async function handleCreateMeeting() {
    libraryRevisionRef.current += 1;
    const generation = ++meetingOpenGenerationRef.current;
    try {
      const meeting = await createMeeting();
      upsertSummary(meeting);
      if (generation === meetingOpenGenerationRef.current) {
        applyActiveMeeting(meeting);
      }
    } catch (error) {
      if (generation === meetingOpenGenerationRef.current) {
        setStatus({ kind: "error", message: String(error) });
      }
    }
  }

  async function handleOpenMeeting(id: number) {
    await openSelectedMeeting(id);
  }

  function openRename(meeting: Pick<PersistedMeeting, "id" | "title">) {
    setRenameTarget(meeting);
    setRenameDraft(meeting.title);
    setRenameError(null);
  }

  function closeRename() {
    setRenameTarget(null);
    setRenameError(null);
  }

  async function handleRename(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!renameTarget) return;
    const title = renameDraft.trim();
    if (!title) {
      setRenameError("Transcription title is required");
      return;
    }
    if (Array.from(title).length > 120) {
      setRenameError("Transcription title must be 120 characters or fewer");
      return;
    }
    try {
      const meeting = await renameMeeting(renameTarget.id, title);
      setMeetingSummaries((previous) =>
        previous.map((summary) =>
          summary.id === meeting.id
            ? { ...summary, title: meeting.title }
            : summary,
        ),
      );
      if (activeMeetingIdRef.current === meeting.id) {
        applyActiveMeeting(meeting);
      }
      closeRename();
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    }
  }

  async function handleDelete() {
    if (!deleteTarget) return;
    const target = deleteTarget;
    const remaining = meetingSummaries.filter(
      (meeting) => meeting.id !== target.id,
    );
    try {
      await deleteMeeting(target.id);
      // A deleted row must never be revived by an earlier open request that
      // resolves after the store has committed this deletion.
      meetingOpenGenerationRef.current += 1;
      const wasActive = activeMeetingIdRef.current === target.id;
      // Deletion has committed. Make the library and dialog reflect that
      // durable boundary before best-effort opening/creating a replacement.
      setMeetingSummaries(remaining);
      setDeleteTarget(null);
      if (!wasActive) return;
      clearActiveMeeting();
      if (wasActive && remaining[0]) {
        await openSelectedMeeting(remaining[0].id);
      } else {
        // Deleting the last meeting seeds a fresh empty one so the workspace
        // always has a real, persisted meeting backing it.
        const generation = ++meetingOpenGenerationRef.current;
        const meeting = await createMeeting();
        upsertSummary(meeting);
        if (generation === meetingOpenGenerationRef.current) {
          applyActiveMeeting(meeting);
        }
      }
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
      // ConfirmDialog owns its in-flight guard. Propagating the failure lets
      // it release that guard while this workspace keeps the error visible.
      throw error;
    }
  }

  async function handleClear() {
    if (!activeMeeting) return;
    const id = activeMeeting.id;
    try {
      await flushMeetingWrites(id);
      const meeting = await clearMeeting(id);
      if (activeMeetingIdRef.current === id) applyActiveMeeting(meeting);
      upsertSummary(meeting);
      setClearPending(false);
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
      setClearPending(false);
      throw error;
    }
  }

  async function handleSave() {
    const base = (fileName ?? "transcript").replace(/\.[^.]+$/, "");
    try {
      await saveTextDialog(
        exportText,
        `${base}.${exportFileExtension(exportFileType)}`,
      );
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    }
  }

  const busy = transcribingId !== null || isGeneratingMfu || isDiarizing;
  const activeIsTranscribing =
    activeMeeting !== null && transcribingId === activeMeeting.id;
  const activeIsGeneratingMfu =
    activeMeeting !== null && generatingMfuId === activeMeeting.id;
  const activeIsDiarizing =
    activeMeeting !== null && diarizingId === activeMeeting.id;
  const hasTranscript = segments.length > 0;
  const meetingTitle = activeMeeting?.title ?? "New Transcription";

  const headerStatus: MeetingStatusView | null = useMemo(() => {
    if (activeIsGeneratingMfu)
      return resolveMeetingStatus(undefined, "crafting");
    if (activeIsDiarizing) return resolveMeetingStatus(undefined, "diarizing");
    if (activeIsTranscribing)
      return resolveMeetingStatus(undefined, transcribingPhase);
    if (transcriptionModelReady === false)
      return resolveMeetingStatus(undefined, "no-model");
    if (status.kind === "error")
      return resolveMeetingStatus(activeMeeting?.status, "error");
    if (!activeMeeting) return null;
    return resolveMeetingStatus(activeMeeting.status);
  }, [
    activeIsGeneratingMfu,
    activeIsDiarizing,
    activeIsTranscribing,
    transcribingPhase,
    transcriptionModelReady,
    status,
    activeMeeting,
  ]);

  function closeSettings() {
    setIsSettingsOpen(false);
    void refreshModelAvailability();
    void refreshExportFileType();
  }

  const modeProps = {
    settingsOpen: isSettingsOpen,
    meetingTranscriptionActive: transcribingId !== null,
    recorderStartPending,
    onRecorderStartPendingChange: setRecorderStartPending,
    isStreamingActive,
    onSelectMeeting: () => setIsRecorderOpen(false),
    onCloseStreaming: () => setIsStreamingOpen(false),
    onSelectStreaming: () => {
      setIsRecorderOpen(false);
      setIsStreamingOpen(true);
    },
    onSelectRecorder: () => {
      setIsStreamingOpen(false);
      setIsRecorderOpen(true);
    },
    onOpenSettings: () => setIsSettingsOpen(true),
    onCloseSettings: closeSettings,
  };
  if (isRecorderOpen) return <AppModeView mode="recorder" {...modeProps} />;
  if (isStreamingOpen) return <AppModeView mode="streaming" {...modeProps} />;
  if (isSettingsOpen) return <AppModeView mode="settings" {...modeProps} />;

  return (
    <div className="app">
      <TranscriptionWorkspace
        sidebarOpen={sidebarOpen}
        setSidebarOpen={setSidebarOpen}
        onCreate={handleCreateMeeting}
        busy={busy}
        onOpenSettings={() => setIsSettingsOpen(true)}
        meetingTitle={meetingTitle}
        activeMeeting={activeMeeting}
        activeIsTranscribing={activeIsTranscribing}
        activeIsGeneratingMfu={activeIsGeneratingMfu}
        activeIsDiarizing={activeIsDiarizing}
        isGeneratingMfu={isGeneratingMfu}
        openRename={openRename}
        onDeleteRequest={setDeleteTarget}
        headerStatus={headerStatus}
        transcribingProgress={transcribingProgress}
        elapsed={elapsed}
        transcribingPhase={transcribingPhase}
        onTranscribe={handleTranscribe}
        isStreamingActive={isStreamingActive}
        transcriptionModelReady={transcriptionModelReady}
        onGenerateMfu={handleGenerateMfu}
        hasTranscript={hasTranscript}
        llmModelReady={llmModelReady}
        exportText={exportText}
        status={status}
        onStatusError={(message) => setStatus({ kind: "error", message })}
        onSave={handleSave}
        onClearRequest={() => setClearPending(true)}
        mfu={mfu}
        fileName={fileName}
        onChooseFile={handleChooseFile}
        onRemoveFile={handleRemoveFile}
        durationLabel={durationLabel}
        setIsStreamingOpen={setIsStreamingOpen}
        setIsRecorderOpen={setIsRecorderOpen}
        meetingSearch={meetingSearch}
        setMeetingSearch={setMeetingSearch}
        meetingSummaries={meetingSummaries}
        filteredMeetingSummaries={filteredMeetingSummaries}
        transcribingId={transcribingId}
        diarizingId={diarizingId}
        generatingMfuId={generatingMfuId}
        onOpenMeeting={handleOpenMeeting}
        segments={segments}
        editingSegmentIndex={editingSegmentIndex}
        setEditingSegmentIndex={setEditingSegmentIndex}
        resolveSpeakerLabel={resolveSpeakerLabel}
        renameSpeaker={renameSpeaker}
        editSegment={editSegment}
        diarizationModelReady={diarizationModelReady}
        onDiarize={handleDiarize}
        mfuPanelVisible={mfuPanelVisible}
        onToggleMfuPanel={handleToggleMfuPanel}
        editMfuField={editMfuField}
      />

      <MeetingDialogs
        renameTarget={renameTarget}
        renameDraft={renameDraft}
        renameError={renameError}
        onRenameDraftChange={(value) => {
          setRenameDraft(value);
          setRenameError(null);
        }}
        onRename={handleRename}
        onCloseRename={closeRename}
        deleteTarget={deleteTarget}
        onDeleteCancel={() => setDeleteTarget(null)}
        onDelete={handleDelete}
        clearPending={clearPending}
        activeMeeting={activeMeeting}
        onClearCancel={() => setClearPending(false)}
        onClear={handleClear}
        diarizationWarning={diarizationWarning}
        onDismissDiarizationWarning={() => setDiarizationWarning(null)}
      />
    </div>
  );
}
