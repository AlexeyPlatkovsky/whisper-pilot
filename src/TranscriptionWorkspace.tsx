import type { Dispatch, SetStateAction } from "react";
import type { Meeting, MeetingMfu, MeetingSummary, Segment } from "./ipc";
import { collapseToBubble } from "./ipc";
import { ModeToggle } from "./ModeToggle";
import { ToggleSwitch } from "./ToggleSwitch";
import { AppLogo, Icon } from "./Icon";
import { ActionIcon } from "./ActionIcon";
import { CopyButton } from "./CopyButton";
import { SpeakerLabelEditor } from "./SpeakerLabelEditor";
import { MeetingRow } from "./MeetingRow";
import { speakerColorClass } from "./speakerColors";
import { formatClock, formatDuration, formatRange } from "./format";
import { formatDetectedLanguage } from "./meetingUtils";
import { resolveMeetingStatus, type MeetingStatusView } from "./meetingStatus";
import { t } from "./i18n";

type TranscriptionWorkspaceProps = {
  sidebarOpen: boolean;
  setSidebarOpen: Dispatch<SetStateAction<boolean>>;
  onCreate: () => void;
  busy: boolean;
  onOpenSettings: () => void;
  meetingTitle: string;
  activeMeeting: Meeting | null;
  activeIsTranscribing: boolean;
  activeIsGeneratingMfu: boolean;
  activeIsDiarizing: boolean;
  isGeneratingMfu: boolean;
  openRename: (meeting: Pick<Meeting, "id" | "title">) => void;
  onDeleteRequest: (meeting: MeetingSummary) => void;
  headerStatus: MeetingStatusView | null;
  transcribingProgress: number | null;
  elapsed: number;
  transcribingPhase: "transcribing" | "diarizing";
  onTranscribe: () => void;
  isStreamingActive: boolean;
  transcriptionModelReady: boolean | null;
  onGenerateMfu: () => Promise<void>;
  hasTranscript: boolean;
  llmModelReady: boolean | null;
  exportText: string;
  status: { kind: "idle" } | { kind: "error"; message: string };
  onStatusError: (message: string) => void;
  onSave: () => void;
  onClearRequest: () => void;
  mfu: MeetingMfu | null;
  fileName: string | null;
  onChooseFile: () => void;
  onRemoveFile: () => void;
  durationLabel: string;
  setIsStreamingOpen: Dispatch<SetStateAction<boolean>>;
  setIsRecorderOpen: Dispatch<SetStateAction<boolean>>;
  meetingSearch: string;
  setMeetingSearch: Dispatch<SetStateAction<string>>;
  meetingSummaries: MeetingSummary[];
  filteredMeetingSummaries: MeetingSummary[];
  transcribingId: number | null;
  diarizingId: number | null;
  generatingMfuId: number | null;
  onOpenMeeting: (id: number) => Promise<void>;
  segments: Segment[];
  editingSegmentIndex: number | null;
  setEditingSegmentIndex: Dispatch<SetStateAction<number | null>>;
  resolveSpeakerLabel: (id: number) => string;
  renameSpeaker: (id: number, label: string) => void;
  editSegment: (index: number, value: string) => void;
  diarizationModelReady: boolean | null;
  onDiarize: () => Promise<void>;
  mfuPanelVisible: boolean;
  onToggleMfuPanel: (checked: boolean) => void;
  editMfuField: (field: keyof MeetingMfu, value: string) => void;
};

/**
 * The Transcription workspace is kept separate from the application shell so
 * that navigation/settings lifecycle state does not obscure the editing UI.
 */
export function TranscriptionWorkspace({
  sidebarOpen,
  setSidebarOpen,
  onCreate: handleCreateMeeting,
  busy,
  onOpenSettings,
  meetingTitle,
  activeMeeting,
  activeIsTranscribing,
  activeIsGeneratingMfu,
  activeIsDiarizing,
  isGeneratingMfu,
  openRename,
  onDeleteRequest: setDeleteTarget,
  headerStatus,
  transcribingProgress,
  elapsed,
  transcribingPhase,
  onTranscribe: handleTranscribe,
  isStreamingActive,
  transcriptionModelReady,
  onGenerateMfu: handleGenerateMfu,
  hasTranscript,
  llmModelReady,
  exportText,
  status,
  onStatusError,
  onSave: handleSave,
  onClearRequest,
  mfu,
  fileName,
  onChooseFile: handleChooseFile,
  onRemoveFile: handleRemoveFile,
  durationLabel,
  setIsStreamingOpen,
  setIsRecorderOpen,
  meetingSearch,
  setMeetingSearch,
  meetingSummaries,
  filteredMeetingSummaries,
  transcribingId,
  diarizingId,
  generatingMfuId,
  onOpenMeeting: handleOpenMeeting,
  segments,
  editingSegmentIndex,
  setEditingSegmentIndex,
  resolveSpeakerLabel,
  renameSpeaker,
  editSegment,
  diarizationModelReady,
  onDiarize: handleDiarize,
  mfuPanelVisible,
  onToggleMfuPanel: handleToggleMfuPanel,
  editMfuField,
}: TranscriptionWorkspaceProps) {
  return (
    <>
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
                onClick={() => setSidebarOpen((v) => !v)}
              >
                <Icon name="panel-left" size={18} />
              </button>
              <span className="wp-sep" />
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="New transcription"
                title="New transcription"
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
                onClick={onOpenSettings}
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
              aria-label="Rename transcription"
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
              aria-label="Delete transcription"
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
                isStreamingActive ||
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
              onError={onStatusError}
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
              icon="eraser"
              label="Clear transcription"
              onClick={onClearRequest}
              disabled={
                !activeMeeting ||
                (!hasTranscript && !mfu) ||
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
              activeIsDiarizing ||
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
              onSelectRecorder={() => setIsRecorderOpen(true)}
            />
            <div className="wp-search">
              <Icon name="search" size={16} />
              <input
                type="search"
                className="wp-search-input"
                placeholder="Search transcriptions..."
                aria-label="Search transcriptions"
                value={meetingSearch}
                onChange={(event) => setMeetingSearch(event.target.value)}
              />
            </div>
            {/* The empty-state copy stays outside the list: a list may only
                own list items. */}
            {meetingSummaries.length === 0 ? (
              <p className="wp-info-muted">No transcriptions yet</p>
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
                        {editingSegmentIndex === i ? (
                          <textarea
                            autoFocus
                            className="wp-speaker-text"
                            value={seg.text}
                            rows={1}
                            aria-label={`Transcript segment ${i + 1}`}
                            onChange={(e) => editSegment(i, e.target.value)}
                            onBlur={() => setEditingSegmentIndex(null)}
                            disabled={
                              activeIsTranscribing ||
                              isGeneratingMfu ||
                              activeIsDiarizing
                            }
                          />
                        ) : (
                          <p
                            className="wp-speaker-text wp-speaker-text--display"
                            role="textbox"
                            aria-label={`Transcript segment ${i + 1}`}
                            aria-readonly="true"
                            tabIndex={0}
                            onClick={() => {
                              const selection = window.getSelection();
                              if (
                                (!selection || selection.isCollapsed) &&
                                !activeIsTranscribing &&
                                !isGeneratingMfu &&
                                !activeIsDiarizing
                              ) {
                                setEditingSegmentIndex(i);
                              }
                            }}
                            onKeyDown={(event) => {
                              if (
                                event.key === "Enter" &&
                                !activeIsTranscribing &&
                                !isGeneratingMfu &&
                                !activeIsDiarizing
                              ) {
                                event.preventDefault();
                                setEditingSegmentIndex(i);
                              }
                            }}
                          >
                            {seg.text}
                          </p>
                        )}
                      </div>
                    </div>
                  );
                })}
            </div>
          </div>

          {/* MFU (summary) panel — hidden by the header switch (WP-96); the
              transcript panel above fills the freed width via its existing
              flex:1 in .wp-transcript-panel. */}
          {mfuPanelVisible && (
            <aside className="wp-mfu">
              {mfu ? (
                <div className="wp-mfu-content wp-mfu-mfu">
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
    </>
  );
}
