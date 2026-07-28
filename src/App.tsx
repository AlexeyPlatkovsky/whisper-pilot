import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  createMeeting,
  deleteMeeting,
  generateNotes,
  getSettings,
  listMeetings,
  listTaskModels,
  openMeeting,
  openFileDialog,
  setMeetingSource,
  transcribeMeeting,
  onTranscriptionPhase,
  saveTextDialog,
  renameMeeting,
  type Meeting as PersistedMeeting,
  type MeetingSummary,
  type MeetingNotes,
  type Segment,
} from "./ipc";
import { SettingsScreen } from "./SettingsScreen";
import { applyTheme, type Theme } from "./theme";
import { t } from "./i18n";
import { formatClock } from "./format";
import { AppLogo, Icon, type IconName } from "./Icon";
import { speakerColorClass, speakerLabel } from "./speakerColors";
import { SpeakerLabelEditor } from "./SpeakerLabelEditor";
import { resolveMeetingStatus, type MeetingStatusView } from "./meetingStatus";

// A running transcription is tracked by meeting id (`transcribingId`), not by
// this union, so that it survives the user switching to another meeting and
// back. This union only carries what the workspace itself is showing.
type Status = { kind: "idle" } | { kind: "error"; message: string };

function formatTime(ms: number): string {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

function formatRange(start: number, end: number): string {
  return `${formatTime(start)} - ${formatTime(end)}`;
}

function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h}h ${m.toString().padStart(2, "0")}m`;
  return `${m}m ${s.toString().padStart(2, "0")}s`;
}

function toSummary(meeting: PersistedMeeting): MeetingSummary {
  return {
    id: meeting.id,
    title: meeting.title,
    created_at_ms: meeting.created_at_ms,
    duration_ms: meeting.duration_ms,
    status: meeting.status,
  };
}

export function App() {
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const [fileName, setFileName] = useState<string | null>(null);
  const [segments, setSegments] = useState<Segment[]>([]);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [elapsed, setElapsed] = useState(0);
  const [transcriptionModelReady, setTranscriptionModelReady] = useState<
    boolean | null
  >(null);
  const [llmModelReady, setLlmModelReady] = useState<boolean | null>(null);
  const [notes, setNotes] = useState<MeetingNotes | null>(null);
  const [speakerLabels, setSpeakerLabels] = useState<Record<number, string>>(
    {},
  );
  const [meetingSummaries, setMeetingSummaries] = useState<MeetingSummary[]>(
    [],
  );
  const [activeMeeting, setActiveMeeting] = useState<PersistedMeeting | null>(
    null,
  );
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
  const [generatingNotesId, setGeneratingNotesId] = useState<number | null>(null);
  const isGeneratingNotes = generatingNotesId !== null;
  // Mirrors the active meeting id for use by async continuations, which would
  // otherwise close over a stale `activeMeeting`.
  const activeMeetingIdRef = useRef<number | null>(null);
  // Mirrors `transcribingId` for the phase-change listener below, which is
  // registered once and would otherwise close over a stale id.
  const transcribingIdRef = useRef<number | null>(null);
  useEffect(() => {
    transcribingIdRef.current = transcribingId;
  }, [transcribingId]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onTranscriptionPhase((event) => {
      if (event.id === transcribingIdRef.current) {
        setTranscribingPhase(event.phase);
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  // Tick a once-per-second elapsed clock while a run is in flight.
  useEffect(() => {
    if (transcribingId === null && generatingNotesId === null) return;
    setElapsed(0);
    const id = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(id);
  }, [transcribingId, generatingNotesId]);

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
    } catch {
      setTranscriptionModelReady(false);
      setLlmModelReady(false);
    }
  }, []);

  useEffect(() => {
    void refreshModelAvailability();
  }, [refreshModelAvailability]);

  useEffect(() => {
    getSettings()
      .then((s) => applyTheme(s.theme as Theme))
      .catch(() => {});
  }, []);

  const applyActiveMeeting = useCallback((meeting: PersistedMeeting) => {
    activeMeetingIdRef.current = meeting.id;
    setActiveMeeting(meeting);
    setFileName(meeting.source_name ?? null);
    setSegments(meeting.segments);
    setSpeakerLabels({});
    setStatus({ kind: "idle" });
    setNotes(meeting.notes ?? null);
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

  const transcriptText = useMemo(
    () =>
      segments
        .map((s) =>
          s.speaker_id !== undefined
            ? `${resolveSpeakerLabel(s.speaker_id)}: ${s.text}`
            : s.text,
        )
        .join("\n"),
    [segments, speakerLabels],
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
      setTranscribingId(null);
    }
  }

  async function handleGenerateNotes() {
    if (!activeMeeting) return;
    const id = activeMeeting.id;
    try {
      setGeneratingNotesId(id);
      setStatus({ kind: "idle" });
      const meeting = await generateNotes(id);
      upsertSummary(meeting);
      if (activeMeetingIdRef.current === meeting.id) {
        setNotes(meeting.notes ?? null);
      }
    } catch (e) {
      if (activeMeetingIdRef.current === id)
        setStatus({ kind: "error", message: String(e) });
    } finally {
      setGeneratingNotesId(null);
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
  }

  async function handleSave() {
    const base = (fileName ?? "transcript").replace(/\.[^.]+$/, "");
    await saveTextDialog(transcriptText, `${base}.txt`);
  }

  // `busy` is global because only one transcription may run at a time: it
  // gates starting another run, anywhere, and — conservatively — creating a
  // meeting, which would move the workspace mid-run. It does NOT gate
  // actions that belong to whichever meeting is on screen — those use
  // `activeIsTranscribing`, so exporting an unrelated finished transcript or
  // attaching a file to an idle meeting still works while a run is going.
  const busy = transcribingId !== null || isGeneratingNotes;
  const activeIsTranscribing =
    activeMeeting !== null && transcribingId === activeMeeting.id;
  const activeIsGeneratingNotes =
    activeMeeting !== null && generatingNotesId === activeMeeting.id;
  const hasTranscript = segments.length > 0;
  const meetingTitle = activeMeeting?.title ?? "New Meeting";

  // The header describes the meeting the user is looking at, through the same
  // resolver the sidebar rows use, so the two can never disagree.
  const headerStatus: MeetingStatusView | null = useMemo(() => {
    if (activeIsGeneratingNotes) return resolveMeetingStatus(undefined, "crafting");
    if (activeIsTranscribing)
      return resolveMeetingStatus(undefined, transcribingPhase);
    if (transcriptionModelReady === false)
      return resolveMeetingStatus(undefined, "no-model");
    if (status.kind === "error")
      return resolveMeetingStatus(activeMeeting?.status, "error");
    if (!activeMeeting) return null;
    return resolveMeetingStatus(activeMeeting.status);
  }, [
    activeIsGeneratingNotes,
    activeIsTranscribing,
    transcribingPhase,
    transcriptionModelReady,
    status,
    activeMeeting,
  ]);

  if (isSettingsOpen) {
    return (
      <div className="app">
        <SettingsScreen
          onClose={() => {
            setIsSettingsOpen(false);
            void refreshModelAvailability();
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
              disabled={!activeMeeting || activeIsTranscribing || isGeneratingNotes}
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
              disabled={!activeMeeting || activeIsTranscribing || isGeneratingNotes}
            >
              <Icon name="trash-2" size={14} />
            </button>
          </div>
        </div>

        <div className="wp-header-right">
          <div className="wp-status" role="status">
            {headerStatus && (
              <>
                {(activeIsTranscribing || activeIsGeneratingNotes) && (
                  <Icon
                    name="refresh-cw"
                    size={14}
                    className={`wp-spin wp-tone--${headerStatus.tone}`}
                  />
                )}
                <span
                  className={`wp-status-label wp-tone--${headerStatus.tone}`}
                >
                  {headerStatus.label}
                </span>
                {(activeIsTranscribing || activeIsGeneratingNotes) && (
                  <span className="wp-status-timer">
                    {formatClock(elapsed)}
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
                transcriptionModelReady !== true
              }
            >
              <Icon name="play" size={17} />
            </button>
            <span className="wp-sep" />
            <ActionIcon icon="refresh-cw" label="Re-run" disabled />
            <span className="wp-sep" />
            <ActionIcon icon="square" label="Stop" disabled />
            <span className="wp-sep" />
            <ActionIcon icon="sparkles" label="Craft notes" accent onClick={() => void handleGenerateNotes()} disabled={!hasTranscript || llmModelReady !== true || activeIsTranscribing || isGeneratingNotes} />
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
            <ActionIcon icon="trash-2" label="Delete transcript" disabled />
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
              isGeneratingNotes ||
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
                disabled={activeIsTranscribing || isGeneratingNotes}
              >
                <Icon name="x" size={12} />
              </button>
            </span>
          ) : (
            <span className="wp-info-muted">No file loaded</span>
          )}
        </div>
        <div className="wp-info-right">
          <span className="wp-info-meta">
            <Icon name="globe" size={14} />
            Russian
          </span>
          <span className="wp-info-meta">{durationLabel}</span>
        </div>
      </div>

      {/* ---- Main content -------------------------------------------------- */}
      <div className="wp-main">
        {sidebarOpen && (
          <aside className="wp-sidebar">
            <div className="wp-search">
              <Icon name="search" size={16} />
              <input
                type="search"
                className="wp-search-input"
                placeholder="Search meetings..."
                aria-label="Search meetings"
              />
            </div>
            {/* The empty-state copy stays outside the list: a list may only
                own list items. */}
            {meetingSummaries.length === 0 ? (
              <p className="wp-info-muted">No meetings yet</p>
            ) : (
              // WebKit drops the implicit list role from a <ul> styled
              // `list-style: none`, and from flex list items — and WKWebView is
              // this app's only runtime. These roles restore the native
              // semantics rather than override them.
              <ul className="wp-meeting-list" role="list">
                {meetingSummaries.map((meeting) => (
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
                        : generatingNotesId === meeting.id
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

              {activeIsTranscribing && (
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
                              disabled={activeIsTranscribing || isGeneratingNotes}
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
                          disabled={activeIsTranscribing || isGeneratingNotes}
                        />
                      </div>
                    </div>
                  );
                })}
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

function ActionIcon({
  icon,
  label,
  accent,
  disabled,
  onClick,
}: {
  icon: IconName;
  label: string;
  accent?: boolean;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      className={`wp-icon-btn${accent ? " wp-icon-btn--accent" : ""}`}
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
    >
      <Icon name={icon} size={17} />
    </button>
  );
}

function MeetingRow({
  title,
  when,
  dur,
  status,
  selected,
  onSelect,
  onRename,
  onDelete,
}: {
  title: string;
  when: string;
  dur: string;
  status: MeetingStatusView;
  selected?: boolean;
  onSelect: () => void;
  onRename: () => void;
  onDelete: () => void;
}) {
  const running = status.tone === "transcribing" || status.tone === "diarizing" || status.tone === "crafting";
  return (
    <li
      className={`wp-meeting-row${selected ? " is-selected" : ""}`}
      role="listitem"
      aria-label={title}
    >
      {/* The dot is the row's whole status surface: colour for a glance, the
          `title` tooltip on hover, and the same words to a screen reader. */}
      <span
        className={`wp-meeting-dot wp-tone--${status.tone}`}
        role="img"
        aria-label={status.label}
        title={status.label}
      />
      <button
        type="button"
        className="wp-meeting-open"
        aria-label={`Open ${title}`}
        aria-current={selected ? "page" : undefined}
        onClick={onSelect}
      >
        <div className="wp-meeting-text">
          {/* Long names are clipped to keep the sidebar at its fixed width, so
              the tooltip is the only way left to read one in full. */}
          <span className="wp-meeting-title" title={title}>
            {title}
          </span>
          <div className="wp-meeting-meta">
            <span>{when}</span>
            <span>{dur}</span>
          </div>
        </div>
      </button>
      <span className="wp-meeting-actions">
        {running ? (
          // While this meeting is transcribing or diarizing, the spinner
          // takes the action group's place — renaming or deleting a running
          // meeting is not something we want to offer mid-run. It is hidden
          // from assistive tech because the dot beside it already announces
          // the current phase; exposing both would name the same status
          // twice per row.
          <span className="wp-meeting-busy" aria-hidden="true">
            <Icon
              name="refresh-cw"
              size={13}
              className={`wp-spin wp-tone--${status.tone}`}
            />
          </span>
        ) : (
          <>
            <button
              type="button"
              aria-label={`Rename ${title}`}
              onClick={onRename}
            >
              <Icon name="pencil" size={13} />
            </button>
            <button
              type="button"
              aria-label={`Delete ${title}`}
              onClick={onDelete}
            >
              <Icon name="trash-2" size={13} />
            </button>
          </>
        )}
      </span>
    </li>
  );
}
