import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  createMeeting,
  deleteMeeting,
  generateMfu,
  getSettings,
  setSetting,
  listMeetings,
  listTaskModels,
  openMeeting,
  openFileDialog,
  setMeetingSource,
  transcribeMeeting,
  diarizeMeeting,
  onTranscriptionPhase,
  onTranscriptionProgress,
  saveTextDialog,
  renameMeeting,
  updateSegment,
  updateMfu,
  type Meeting as PersistedMeeting,
  type MeetingSummary,
  type MeetingMfu,
  type Segment,
} from "./ipc";
import { SettingsScreen } from "./SettingsScreen";
import { StreamingView } from "./StreamingView";
import { ModeToggle } from "./ModeToggle";
import { ToggleSwitch } from "./ToggleSwitch";
import { applyTheme, type Theme } from "./theme";
import { applyStatusColors, parseStatusColors } from "./statusColors";
import { t } from "./i18n";
import { formatClock, formatDuration, formatRange } from "./format";
import { AppLogo, Icon } from "./Icon";
import { ActionIcon } from "./ActionIcon";
import { CopyButton } from "./CopyButton";
import { speakerColorClass, speakerLabel } from "./speakerColors";
import { SpeakerLabelEditor } from "./SpeakerLabelEditor";
import { MeetingRow } from "./MeetingRow";
import { formatDetectedLanguage, toSummary } from "./meetingUtils";
import { resolveMeetingStatus, type MeetingStatusView } from "./meetingStatus";
import {
  renderForExport,
  exportFileExtension,
  type ExportFileType,
} from "./export";

// A running transcription is tracked by meeting id (`transcribingId`), not by
// this union, so that it survives the user switching to another meeting and
// back. This union only carries what the workspace itself is showing.
type Status = { kind: "idle" } | { kind: "error"; message: string };

// How long an edited segment or mfu field waits, idle, before it is
// auto-saved to the database. There is no explicit save action or state.
const AUTOSAVE_DEBOUNCE_MS = 500;

export function App() {
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const [fileName, setFileName] = useState<string | null>(null);
  const [segments, setSegments] = useState<Segment[]>([]);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isStreamingOpen, setIsStreamingOpen] = useState(false);
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
  const [diarizationWarning, setDiarizationWarning] = useState<string | null>(
    null,
  );
  const [exportFileType, setExportFileType] =
    useState<ExportFileType>("plain_text");
  // WP-90: view-only visibility of the MFU (summary) panel, persisted under
  // its own settings key so it survives a restart independently of
  // Streaming's. Defaults ON; a settings read/write failure keeps it ON
  // without surfacing a blocking error (see the getSettings effect below and
  // handleToggleMfuPanel).
  const [mfuPanelVisible, setMfuPanelVisible] = useState(true);
  // The meeting currently being transcribed, or null. Kept outside `status`
  // and outside `activeMeeting` so that opening a different meeting cannot
  // discard a run that is still going.
  const [transcribingId, setTranscribingId] = useState<number | null>(null);
  // Which pass of the in-flight run is showing. Diarization runs after
  // transcription completes, on the same run, so this is reset per run rather
  // than tracked alongside `transcribingId`.
  const [transcribingPhase, setTranscribingPhase] = useState<
    "transcribing" | "diarizing"
  >("transcribing");
  const [transcribingProgress, setTranscribingProgress] = useState<
    number | null
  >(null);
  const [generatingMfuId, setGeneratingMfuId] = useState<number | null>(null);
  const isGeneratingMfu = generatingMfuId !== null;
  // The meeting whose standalone "Diarize" run is in flight, or null — the
  // header action that re-runs speaker identification alone, without
  // re-transcribing (separate from diarization folded into Transcribe).
  const [diarizingId, setDiarizingId] = useState<number | null>(null);
  const isDiarizing = diarizingId !== null;
  const [diarizationModelReady, setDiarizationModelReady] = useState<
    boolean | null
  >(null);
  // Mirrors the active meeting id for use by async continuations, which would
  // otherwise close over a stale `activeMeeting`.
  const activeMeetingIdRef = useRef<number | null>(null);
  // Pending debounced auto-save timers, keyed by segment index, and the one
  // pending mfu auto-save timer. Each timer's meeting id is captured at
  // schedule time, so switching meetings mid-debounce still saves to the
  // meeting the edit actually belongs to.
  const segmentSaveTimers = useRef<Map<number, ReturnType<typeof setTimeout>>>(
    new Map(),
  );
  const mfuSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Mirrors `mfu` for the debounce timer callback below, which fires
  // outside a React batch and must read the latest merged fields rather than
  // whatever was in scope when the timer was scheduled.
  const mfuRef = useRef<MeetingMfu | null>(null);
  useEffect(() => {
    mfuRef.current = mfu;
  }, [mfu]);
  // Mirrors `transcribingId` for the phase-change listener below, which is
  // registered once and would otherwise close over a stale id.
  const transcribingIdRef = useRef<number | null>(null);
  useEffect(() => {
    transcribingIdRef.current = transcribingId;
  }, [transcribingId]);

  // Tick a once-per-second elapsed clock while a run is in flight.
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
      const transcription = models.find((m) => m.id === "transcription");
      setTranscriptionModelReady(transcription?.downloaded ?? false);
      const settings = await getSettings();
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

  // View-only: never gates Craft MFU, Diarize, Transcribe, or any other
  // action. Persistence is best-effort — a write failure leaves the switch
  // exactly as the user set it, with no blocking error (S-1..S-3, DoD 2-3).
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

  const applyActiveMeeting = useCallback((meeting: PersistedMeeting) => {
    activeMeetingIdRef.current = meeting.id;
    setActiveMeeting(meeting);
    setFileName(meeting.source_name ?? null);
    setSegments(meeting.segments);
    setSpeakerLabels({});
    setStatus({ kind: "idle" });
    setMfu(meeting.mfu ?? null);
  }, []);

  const upsertSummary = useCallback((meeting: PersistedMeeting) => {
    setMeetingSummaries((previous) =>
      previous.some((summary) => summary.id === meeting.id)
        ? previous.map((summary) =>
            summary.id === meeting.id ? toSummary(meeting) : summary,
          )
        : [toSummary(meeting), ...previous],
    );
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onTranscriptionPhase((event) => {
      if (event.id !== transcribingIdRef.current) return;

      setTranscribingPhase(event.phase);
      setTranscribingProgress(null);

      // Transcription persists its segments before diarization begins. Reload
      // the meeting so the user can read that completed work immediately.
      if (
        event.phase === "diarizing" &&
        activeMeetingIdRef.current === event.id
      ) {
        void Promise.resolve()
          .then(() => openMeeting(event.id))
          .then((meeting) => {
            if (!meeting) return;
            if (activeMeetingIdRef.current !== meeting.id) return;
            setActiveMeeting(meeting);
            setFileName(meeting.source_name ?? null);
            setSegments(meeting.segments);
            setSpeakerLabels({});
            setMfu(meeting.mfu ?? null);
            upsertSummary(meeting);
          })
          .catch(() => {});
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [upsertSummary]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onTranscriptionProgress((event) => {
      if (event.id !== transcribingIdRef.current) return;
      setTranscribingProgress(Math.max(0, Math.min(100, event.percent)));
    })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let cancelled = false;

    async function loadMeetingLibrary() {
      try {
        const summaries = await listMeetings();
        if (cancelled) return;
        // The workspace is always backed by a real, persisted meeting. When the
        // library is empty we seed one so the initial meeting can be renamed or
        // deleted and never loses its transcript when another meeting is opened.
        if (summaries.length === 0) {
          const meeting = await createMeeting();
          if (cancelled) return;
          setMeetingSummaries([toSummary(meeting)]);
          applyActiveMeeting(meeting);
          return;
        }
        setMeetingSummaries(summaries);
        const meeting = await openMeeting(summaries[0].id);
        if (!cancelled) applyActiveMeeting(meeting);
      } catch (error) {
        if (!cancelled) setStatus({ kind: "error", message: String(error) });
      }
    }

    void loadMeetingLibrary();
    return () => {
      cancelled = true;
    };
  }, [applyActiveMeeting]);

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
    if (!activeMeeting) return;
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
    if (!activeMeeting?.source_path) return;
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
    if (!activeMeeting) return;
    try {
      const meeting = await setMeetingSource(activeMeeting.id, null);
      applyActiveMeeting(meeting);
      upsertSummary(meeting);
    } catch (e) {
      setStatus({ kind: "error", message: String(e) });
    }
  }

  async function handleCreateMeeting() {
    try {
      const meeting = await createMeeting();
      upsertSummary(meeting);
      applyActiveMeeting(meeting);
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    }
  }

  async function handleOpenMeeting(id: number) {
    try {
      applyActiveMeeting(await openMeeting(id));
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    }
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
      setRenameError("Meeting label is required");
      return;
    }
    if (Array.from(title).length > 120) {
      setRenameError("Meeting label must be 120 characters or fewer");
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
      applyActiveMeeting(meeting);
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
      const wasActive = activeMeeting?.id === target.id;
      if (wasActive && remaining[0]) {
        const next = await openMeeting(remaining[0].id);
        setMeetingSummaries(remaining);
        applyActiveMeeting(next);
      } else if (wasActive) {
        // Deleting the last meeting seeds a fresh empty one so the workspace
        // always has a real, persisted meeting backing it.
        const meeting = await createMeeting();
        setMeetingSummaries([toSummary(meeting)]);
        applyActiveMeeting(meeting);
      } else {
        setMeetingSummaries(remaining);
      }
      setDeleteTarget(null);
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    }
  }

  function editSegment(index: number, text: string) {
    setSegments((prev) =>
      prev.map((s, i) => (i === index ? { ...s, text } : s)),
    );
    const meetingId = activeMeeting?.id;
    if (meetingId === undefined) return;
    const pending = segmentSaveTimers.current.get(index);
    if (pending !== undefined) clearTimeout(pending);
    segmentSaveTimers.current.set(
      index,
      setTimeout(() => {
        segmentSaveTimers.current.delete(index);
        updateSegment(meetingId, index, text).catch((error) => {
          setStatus({ kind: "error", message: String(error) });
        });
      }, AUTOSAVE_DEBOUNCE_MS),
    );
  }

  function editMfuField(field: keyof MeetingMfu, value: string) {
    if (field === "meeting_id") return;
    setMfu((prev) => (prev ? { ...prev, [field]: value } : prev));
    const meetingId = activeMeeting?.id;
    if (meetingId === undefined) return;
    if (mfuSaveTimer.current !== null) clearTimeout(mfuSaveTimer.current);
    mfuSaveTimer.current = setTimeout(() => {
      mfuSaveTimer.current = null;
      const current = mfuRef.current;
      if (current) {
        updateMfu({ ...current, meeting_id: meetingId }).catch((error) => {
          setStatus({ kind: "error", message: String(error) });
        });
      }
    }, AUTOSAVE_DEBOUNCE_MS);
  }

  async function handleSave() {
    const base = (fileName ?? "transcript").replace(/\.[^.]+$/, "");
    await saveTextDialog(
      exportText,
      `${base}.${exportFileExtension(exportFileType)}`,
    );
  }

  // `busy` is global because only one transcription may run at a time: it
  // gates starting another run, anywhere, and — conservatively — creating a
  // meeting, which would move the workspace mid-run. It does NOT gate
  // actions that belong to whichever meeting is on screen — those use
  // `activeIsTranscribing`, so exporting an unrelated finished transcript or
  // attaching a file to an idle meeting still works while a run is going.
  const busy = transcribingId !== null || isGeneratingMfu || isDiarizing;
  const activeIsTranscribing =
    activeMeeting !== null && transcribingId === activeMeeting.id;
  const activeIsGeneratingMfu =
    activeMeeting !== null && generatingMfuId === activeMeeting.id;
  const activeIsDiarizing =
    activeMeeting !== null && diarizingId === activeMeeting.id;
  const hasTranscript = segments.length > 0;
  const meetingTitle = activeMeeting?.title ?? "New Meeting";

  // The header describes the meeting the user is looking at, through the same
  // resolver the sidebar rows use, so the two can never disagree.
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

  // Streaming is checked first: it renders its own top-level view as a
  // mounted child, and a session may be actively recording in the backend
  // while Settings opens on top of it. Replacing that child with
  // SettingsScreen (as the Meeting branch below does, safely, since Meeting's
  // own state lives in this component rather than a child) would unmount it
  // and drop its live session state, so Settings layers as an overlay here
  // instead of swapping the tree.
  if (isStreamingOpen) {
    return (
      <>
        <StreamingView
          onClose={() => setIsStreamingOpen(false)}
          onOpenSettings={() => setIsSettingsOpen(true)}
        />
        {isSettingsOpen && (
          <div className="settings-overlay">
            <SettingsScreen
              onClose={() => {
                setIsSettingsOpen(false);
                void refreshModelAvailability();
                void refreshExportFileType();
              }}
            />
          </div>
        )}
      </>
    );
  }

  if (isSettingsOpen) {
    return (
      <div className="app">
        <SettingsScreen
          onClose={() => {
            setIsSettingsOpen(false);
            void refreshModelAvailability();
            void refreshExportFileType();
          }}
        />
      </div>
    );
  }

  return (
    <div className="app">
      {/* ---- Top header (shares the row with the macOS traffic lights, which
          the OS draws via the Overlay titleBarStyle — we reserve space for
          them on the left rather than drawing our own) ------------------------ */}
      <header className="wp-header" data-tauri-drag-region="deep">
        <div className="wp-header-lead">
          <div className="wp-header-left">
            {/* Reserved gap for the OS traffic lights (close/min/max) */}
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
                aria-label="New meeting"
                title="New meeting"
                onClick={handleCreateMeeting}
                disabled={busy}
              >
                <Icon name="plus" size={18} />
              </button>
              <span className="wp-sep" />
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="Settings"
                onClick={() => setIsSettingsOpen(true)}
              >
                <Icon name="settings" size={18} />
              </button>
            </div>
          </div>

          <div className="wp-title-group">
            <h1 className="wp-title">{meetingTitle}</h1>
            <button
              type="button"
              className="wp-icon-btn wp-icon-btn--ghost"
              aria-label="Rename meeting"
              onClick={() => activeMeeting && openRename(activeMeeting)}
              disabled={
                !activeMeeting ||
                activeIsTranscribing ||
                isGeneratingMfu ||
                activeIsDiarizing
              }
            >
              <Icon name="pencil" size={14} />
            </button>
            <button
              type="button"
              className="wp-icon-btn wp-icon-btn--ghost"
              aria-label="Delete meeting"
              onClick={() =>
                activeMeeting &&
                setDeleteTarget({
                  id: activeMeeting.id,
                  title: activeMeeting.title,
                  created_at_ms: activeMeeting.created_at_ms,
                  duration_ms: activeMeeting.duration_ms,
                  status: activeMeeting.status,
                })
              }
              disabled={
                !activeMeeting ||
                activeIsTranscribing ||
                isGeneratingMfu ||
                activeIsDiarizing
              }
            >
              <Icon name="trash-2" size={14} />
            </button>
          </div>
        </div>

        <div className="wp-header-right">
          <div className="wp-status" role="status">
            {headerStatus && (
              <>
                <Icon
                  name={headerStatus.icon}
                  size={14}
                  className={`${activeIsTranscribing || activeIsGeneratingMfu || activeIsDiarizing ? "wp-spin " : ""}wp-tone--${headerStatus.tone} wp-status--${headerStatus.statusKey}`}
                />
                <span
                  className={`wp-status-label wp-tone--${headerStatus.tone} wp-status--${headerStatus.statusKey}`}
                >
                  {headerStatus.label}
                </span>
                {(activeIsTranscribing ||
                  activeIsGeneratingMfu ||
                  activeIsDiarizing) && (
                  <span className="wp-status-timer">
                    {formatClock(elapsed)}
                  </span>
                )}
                {activeIsTranscribing && transcribingProgress !== null && (
                  <span className="wp-status-progress">
                    <progress
                      className="wp-status-progress-bar"
                      value={transcribingProgress}
                      max={100}
                      aria-label="Transcription progress"
                    />
                    <span className="wp-status-progress-label">
                      {transcribingProgress}%
                    </span>
                  </span>
                )}
              </>
            )}
          </div>

          <div className="wp-action-group">
            <button
              type="button"
              className="wp-icon-btn"
              aria-label="Transcribe"
              title="Transcribe"
              onClick={handleTranscribe}
              disabled={
                busy ||
                !activeMeeting?.source_path ||
                activeMeeting?.source_missing ||
                transcriptionModelReady !== true
              }
            >
              <Icon name="play" size={17} />
            </button>
            <span className="wp-sep" />
            <ActionIcon
              icon="sparkles"
              label="Craft MFU"
              accent
              onClick={() => void handleGenerateMfu()}
              disabled={
                !hasTranscript ||
                llmModelReady !== true ||
                activeIsTranscribing ||
                isGeneratingMfu ||
                activeIsDiarizing
              }
            />
            <span className="wp-sep" />
            <CopyButton
              text={exportText}
              resetKey={activeMeeting?.id ?? null}
              onError={(message) => setStatus({ kind: "error", message })}
              disabled={activeIsTranscribing || !hasTranscript}
            />
            <span className="wp-sep" />
            <button
              type="button"
              className="wp-icon-btn"
              aria-label={t("save")}
              title={t("save")}
              onClick={handleSave}
              disabled={activeIsTranscribing || !hasTranscript}
            >
              <Icon name="download" size={17} />
            </button>
            <span className="wp-sep" />
            <ActionIcon
              icon="trash-2"
              label="Delete active meeting"
              onClick={() =>
                activeMeeting &&
                setDeleteTarget({
                  id: activeMeeting.id,
                  title: activeMeeting.title,
                  created_at_ms: activeMeeting.created_at_ms,
                  duration_ms: activeMeeting.duration_ms,
                  status: activeMeeting.status,
                })
              }
              disabled={
                !activeMeeting ||
                activeIsTranscribing ||
                isGeneratingMfu ||
                activeIsDiarizing
              }
            />
          </div>
        </div>
      </header>

      {/* ---- Meeting info bar ---------------------------------------------- */}
      <div className="wp-info-bar">
        <div className="wp-info-left">
          <span className="wp-info-label">Files:</span>
          <button
            type="button"
            className="wp-icon-btn wp-info-add"
            aria-label="Choose file"
            title="Choose an audio or video file"
            onClick={handleChooseFile}
            disabled={
              activeIsTranscribing ||
              isGeneratingMfu ||
              !activeMeeting ||
              transcriptionModelReady !== true
            }
          >
            <Icon name="folder" size={16} />
          </button>
          {fileName ? (
            <span className="wp-file-chip">
              {fileName}
              <button
                type="button"
                className="wp-icon-btn wp-icon-btn--tiny"
                aria-label="Remove file"
                onClick={handleRemoveFile}
                disabled={
                  activeIsTranscribing || isGeneratingMfu || activeIsDiarizing
                }
              >
                <Icon name="x" size={12} />
              </button>
            </span>
          ) : (
            <span className="wp-info-muted">No file loaded</span>
          )}
          {activeMeeting?.source_missing && (
            <span className="wp-info-warning" role="status">
              Source file missing — re-transcribe disabled. The transcript and
              mfu are still readable and editable.
            </span>
          )}
        </div>
        <div className="wp-info-right">
          {activeMeeting?.status === "finished" && (
            <span className="wp-info-meta">
              <Icon name="globe" size={14} />
              {formatDetectedLanguage(activeMeeting.language)}
            </span>
          )}
          <span className="wp-info-meta">{durationLabel}</span>
        </div>
      </div>

      {/* ---- Main content -------------------------------------------------- */}
      <div className="wp-main">
        {sidebarOpen && (
          <aside className="wp-sidebar">
            <ModeToggle
              mode="meeting"
              onSelectMeeting={() => {}}
              onSelectStreaming={() => setIsStreamingOpen(true)}
            />
            <div className="wp-search">
              <Icon name="search" size={16} />
              <input
                type="search"
                className="wp-search-input"
                placeholder="Search meetings..."
                aria-label="Search meetings"
                value={meetingSearch}
                onChange={(event) => setMeetingSearch(event.target.value)}
              />
            </div>
            {/* The empty-state copy stays outside the list: a list may only
                own list items. */}
            {meetingSummaries.length === 0 ? (
              <p className="wp-info-muted">No meetings yet</p>
            ) : filteredMeetingSummaries.length === 0 ? (
              <p className="wp-info-muted">No matches</p>
            ) : (
              // WebKit drops the implicit list role from a <ul> styled
              // `list-style: none`, and from flex list items — and WKWebView is
              // this app's only runtime. These roles restore the native
              // semantics rather than override them.
              <ul className="wp-meeting-list" role="list">
                {filteredMeetingSummaries.map((meeting) => (
                  <MeetingRow
                    key={meeting.id}
                    title={meeting.title}
                    when={new Date(meeting.created_at_ms).toLocaleDateString()}
                    dur={
                      meeting.duration_ms
                        ? formatDuration(meeting.duration_ms)
                        : "—"
                    }
                    status={resolveMeetingStatus(
                      meeting.status,
                      transcribingId === meeting.id
                        ? transcribingPhase
                        : diarizingId === meeting.id
                          ? "diarizing"
                          : generatingMfuId === meeting.id
                            ? "crafting"
                            : "none",
                    )}
                    selected={activeMeeting?.id === meeting.id}
                    onSelect={() => void handleOpenMeeting(meeting.id)}
                    onRename={() => openRename(meeting)}
                    onDelete={() => setDeleteTarget(meeting)}
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
                <span className="wp-transcript-meta">
                  {hasTranscript
                    ? `${segments.length} segment${segments.length === 1 ? "" : "s"}`
                    : "No segments"}
                </span>
              </div>
              <div className="wp-transcript-actions">
                <Icon name="pencil" size={14} />
                <span>Editable</span>
                <span className="wp-sep" />
                <ActionIcon
                  icon="messages-square"
                  label="Diarize speakers"
                  onClick={() => void handleDiarize()}
                  disabled={
                    !hasTranscript ||
                    activeMeeting?.source_missing ||
                    diarizationModelReady !== true ||
                    busy
                  }
                />
                <span className="wp-sep" />
                <span className="wp-mfu-toggle-label">MFU</span>
                <ToggleSwitch
                  checked={mfuPanelVisible}
                  onChange={handleToggleMfuPanel}
                  label="MFU panel"
                />
              </div>
            </div>
            <div className="wp-separator" />

            <div className="wp-transcript-content">
              {transcriptionModelReady === false && (
                <div className="wp-notice wp-notice--error">
                  {t("modelMissing")}
                </div>
              )}

              {status.kind === "error" && (
                <div className="wp-notice wp-notice--error" role="alert">
                  {status.message}
                </div>
              )}

              {activeIsTranscribing && !hasTranscript && (
                <div className="wp-empty">
                  <p>
                    {transcribingPhase === "diarizing"
                      ? "Diarizing…"
                      : "Transcribing…"}
                  </p>
                </div>
              )}

              {!activeIsTranscribing &&
                !hasTranscript &&
                status.kind !== "error" && (
                  <div className="wp-empty">
                    <p>{t("emptyState")}</p>
                  </div>
                )}

              {hasTranscript &&
                segments.map((seg, i) => {
                  const hasSpeaker = seg.speaker_id !== undefined;
                  return (
                    <div className="wp-speaker-block" key={i}>
                      <span
                        className={`wp-speaker-bar ${hasSpeaker ? speakerColorClass(seg.speaker_id!) : "wp-speaker-bar--none"}`}
                      />
                      <div className="wp-speaker-body">
                        <div className="wp-speaker-head">
                          {hasSpeaker && (
                            <SpeakerLabelEditor
                              speakerId={seg.speaker_id!}
                              label={resolveSpeakerLabel(seg.speaker_id!)}
                              onRename={renameSpeaker}
                              disabled={
                                activeIsTranscribing ||
                                isGeneratingMfu ||
                                activeIsDiarizing
                              }
                            />
                          )}
                          <span className="wp-speaker-time">
                            {formatRange(seg.start_ms, seg.end_ms)}
                          </span>
                        </div>
                        <textarea
                          className="wp-speaker-text"
                          value={seg.text}
                          rows={1}
                          onChange={(e) => editSegment(i, e.target.value)}
                          disabled={
                            activeIsTranscribing ||
                            isGeneratingMfu ||
                            activeIsDiarizing
                          }
                        />
                      </div>
                    </div>
                  );
                })}
            </div>
          </div>

          {/* MFU (summary) panel — hidden by the header switch (WP-90); the
              transcript panel above fills the freed width via its existing
              flex:1 in .wp-transcript-panel. */}
          {mfuPanelVisible && (
            <aside className="wp-mfu">
              {mfu ? (
                <div className="wp-mfu-mfu">
                  {(
                    [
                      ["summary", "Summary"],
                      ["decisions", "Decisions"],
                      ["action_items", "Action Items"],
                      ["open_questions", "Open Questions"],
                      ["participants", "Participants"],
                    ] as const
                  ).map(([field, heading]) => (
                    <section className="wp-mfu-section" key={field}>
                      <h3 className="wp-mfu-heading">{heading}</h3>
                      <textarea
                        className="wp-mfu-text wp-mfu-textarea"
                        value={mfu[field]}
                        rows={1}
                        onChange={(e) => editMfuField(field, e.target.value)}
                        disabled={
                          activeIsTranscribing ||
                          isGeneratingMfu ||
                          activeIsDiarizing
                        }
                      />
                    </section>
                  ))}
                </div>
              ) : (
                <div className="wp-mfu-placeholder">
                  <div className="wp-mfu-icon-frame">
                    <Icon name="sparkles" size={32} />
                  </div>
                  <p className="wp-mfu-title">Run MFU Craft</p>
                  <p className="wp-mfu-subtitle">
                    Generate summary, decisions, action items, and open
                    questions.
                  </p>
                </div>
              )}
            </aside>
          )}
        </section>
      </div>

      {renameTarget && (
        <div className="modal-overlay">
          <form
            className="modal-panel confirm-modal"
            role="dialog"
            aria-modal="true"
            aria-label="Rename meeting"
            onSubmit={handleRename}
            onKeyDown={(event) => {
              if (event.key === "Escape") closeRename();
            }}
          >
            <div className="modal-header">
              <span className="modal-title">Rename meeting</span>
            </div>
            <label htmlFor="meeting-label">Meeting label</label>
            <input
              id="meeting-label"
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
              This permanently removes the meeting and its transcript.
            </p>
            <div className="confirm-actions">
              <button type="button" onClick={() => setDeleteTarget(null)}>
                Cancel
              </button>
              <button type="button" onClick={() => void handleDelete()}>
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
      {diarizationWarning && (
        <div className="modal-overlay">
          <div
            className="modal-panel confirm-modal"
            role="alertdialog"
            aria-modal="true"
            aria-label="Speaker identification issue"
            onKeyDown={(event) => {
              if (event.key === "Escape") setDiarizationWarning(null);
            }}
          >
            <div className="modal-header">
              <span className="modal-title">Speaker identification issue</span>
            </div>
            <p className="confirm-warning">{diarizationWarning}</p>
            <div className="confirm-actions">
              <button
                type="button"
                autoFocus
                onClick={() => setDiarizationWarning(null)}
              >
                OK
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
