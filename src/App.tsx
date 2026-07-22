import { useEffect, useMemo, useState } from "react";
import {
  getSettings,
  listTaskModels,
  openFileDialog,
  transcribeFile,
  saveTextDialog,
  type Segment,
} from "./ipc";
import { SettingsScreen } from "./SettingsScreen";
import { applyTheme, type Theme } from "./theme";
import { t } from "./i18n";
import { formatClock } from "./format";
import { AppLogo, Icon, type IconName } from "./Icon";

type Status =
  | { kind: "idle" }
  | { kind: "transcribing"; file: string }
  | { kind: "error"; message: string };

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

// Presentational sidebar sample — the meeting library (persistence) is not built
// in M1. Rendered as static scaffolding to match the pencil design; the selected
// row reflects the currently loaded file when one is present.
const SAMPLE_MEETINGS = [
  { title: "Product Standup", when: "Yesterday", dur: "18m", dot: "ok" },
  { title: "Design Review", when: "Jul 19", dur: "55m", dot: "ok" },
  {
    title: "Client Call - Acme",
    when: "Jul 18",
    dur: "1h 12m",
    dot: "progress",
    status: "Transcribing",
  },
  { title: "Sprint Retrospective", when: "Jul 17", dur: "37m", dot: "ok" },
  {
    title: "Architecture Sync",
    when: "Jul 16",
    dur: "1h 05m",
    dot: "error",
    status: "MFU Failed",
  },
] as const;

const SPEAKER_ACCENT = ["accent", "success", "warning"] as const;

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

  // Tick a once-per-second elapsed clock while a transcription is running.
  useEffect(() => {
    if (status.kind !== "transcribing") return;
    setElapsed(0);
    const id = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(id);
  }, [status.kind]);

  async function refreshModelAvailability() {
    try {
      const models = await listTaskModels();
      const transcription = models.find((m) => m.id === "transcription");
      setTranscriptionModelReady(transcription?.downloaded ?? false);
    } catch {
      setTranscriptionModelReady(false);
    }
  }

  useEffect(() => {
    void refreshModelAvailability();
  }, []);

  useEffect(() => {
    getSettings()
      .then((s) => applyTheme(s.theme as Theme))
      .catch(() => {});
  }, []);

  const transcriptText = useMemo(
    () => segments.map((s) => s.text).join("\n"),
    [segments],
  );

  const durationLabel = useMemo(() => {
    if (segments.length === 0) return "—";
    return formatDuration(segments[segments.length - 1].end_ms);
  }, [segments]);

  async function handleAddFile() {
    try {
      const path = await openFileDialog();
      if (!path) return;
      const shortName = path.split("/").pop() ?? path;
      setStatus({ kind: "transcribing", file: shortName });
      setSegments([]);
      setFileName(shortName);
      const result = await transcribeFile(path, "ru");
      setSegments(result.segments);
      setFileName(result.file_name);
      setStatus({ kind: "idle" });
    } catch (e) {
      setStatus({ kind: "error", message: String(e) });
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

  const busy = status.kind === "transcribing";
  const hasTranscript = segments.length > 0;
  const meetingTitle = fileName
    ? fileName.replace(/\.[^.]+$/, "")
    : "New Meeting";

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
      <header className="wp-header" data-tauri-drag-region>
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
                aria-label={t("addFile")}
                title={t("addFile")}
                onClick={handleAddFile}
                disabled={busy || transcriptionModelReady !== true}
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
              disabled
            >
              <Icon name="pencil" size={14} />
            </button>
            <button
              type="button"
              className="wp-icon-btn wp-icon-btn--ghost"
              aria-label="Delete meeting"
              disabled
            >
              <Icon name="trash-2" size={14} />
            </button>
          </div>
        </div>

        <div className="wp-header-right">
          <div className="wp-status" role="status">
            {busy ? (
              <>
                <Icon name="refresh-cw" size={14} className="wp-spin" />
                <span className="wp-status-label">
                  {t("transcribingPrefix")}
                </span>
                <span className="wp-status-timer">{formatClock(elapsed)}</span>
              </>
            ) : status.kind === "error" ? (
              <span className="wp-status-error">Error</span>
            ) : (
              <span className="wp-status-idle">
                {hasTranscript ? "Ready" : "Idle"}
              </span>
            )}
          </div>

          <div className="wp-action-group">
            <ActionIcon icon="play" label="Start" disabled />
            <span className="wp-sep" />
            <ActionIcon icon="refresh-cw" label="Re-run" disabled />
            <span className="wp-sep" />
            <ActionIcon icon="square" label="Stop" disabled />
            <span className="wp-sep" />
            <ActionIcon icon="sparkles" label="Craft notes" accent disabled />
            <span className="wp-sep" />
            <button
              type="button"
              className="wp-icon-btn"
              aria-label={t("save")}
              title={t("save")}
              onClick={handleSave}
              disabled={busy || !hasTranscript}
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
            onClick={handleAddFile}
            disabled={busy || transcriptionModelReady !== true}
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
                onClick={() => {
                  setFileName(null);
                  setSegments([]);
                  setStatus({ kind: "idle" });
                }}
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
            <div className="wp-meeting-list">
              <MeetingRow
                title={fileName ? meetingTitle : "Quarterly Planning Sync"}
                when="Today"
                dur={hasTranscript ? durationLabel : "42m"}
                dot="ok"
                selected
              />
              {SAMPLE_MEETINGS.map((m) => (
                <MeetingRow
                  key={m.title}
                  title={m.title}
                  when={m.when}
                  dur={m.dur}
                  dot={m.dot}
                  status={"status" in m ? m.status : undefined}
                />
              ))}
            </div>
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
                <div className="wp-notice wp-notice--error">
                  {status.message}
                </div>
              )}

              {busy && (
                <div className="wp-empty">
                  <p>Transcribing…</p>
                </div>
              )}

              {!busy && !hasTranscript && status.kind !== "error" && (
                <div className="wp-empty">
                  <p>{t("emptyState")}</p>
                </div>
              )}

              {hasTranscript &&
                segments.map((seg, i) => (
                  <div className="wp-speaker-block" key={i}>
                    <span
                      className={`wp-speaker-bar wp-speaker-bar--${SPEAKER_ACCENT[i % SPEAKER_ACCENT.length]}`}
                    />
                    <div className="wp-speaker-body">
                      <div className="wp-speaker-head">
                        <span className="wp-speaker-name">
                          Speaker {(i % SPEAKER_ACCENT.length) + 1}
                        </span>
                        <span className="wp-speaker-time">
                          {formatRange(seg.start_ms, seg.end_ms)}
                        </span>
                      </div>
                      <textarea
                        className="wp-speaker-text"
                        value={seg.text}
                        rows={1}
                        onChange={(e) => editSegment(i, e.target.value)}
                      />
                    </div>
                  </div>
                ))}
            </div>
          </div>

          {/* MFU (summary) panel — summarization is not wired in M1; this is a
              presentational placeholder matching the pencil design. */}
          <aside className="wp-mfu">
            <div className="wp-mfu-placeholder">
              <div className="wp-mfu-icon-frame">
                <Icon name="sparkles" size={32} />
              </div>
              <p className="wp-mfu-title">Run MFU Craft</p>
              <p className="wp-mfu-subtitle">
                Generate summary, decisions, action items, and open questions.
              </p>
            </div>
          </aside>
        </section>
      </div>
    </div>
  );
}

function ActionIcon({
  icon,
  label,
  accent,
  disabled,
}: {
  icon: IconName;
  label: string;
  accent?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className={`wp-icon-btn${accent ? " wp-icon-btn--accent" : ""}`}
      aria-label={label}
      title={label}
      disabled={disabled}
    >
      <Icon name={icon} size={17} />
    </button>
  );
}

function MeetingRow({
  title,
  when,
  dur,
  dot,
  status,
  selected,
}: {
  title: string;
  when: string;
  dur: string;
  dot: string;
  status?: string;
  selected?: boolean;
}) {
  return (
    <div className={`wp-meeting-row${selected ? " is-selected" : ""}`}>
      <span className={`wp-meeting-dot wp-meeting-dot--${dot}`} />
      <div className="wp-meeting-text">
        <span className="wp-meeting-title">{title}</span>
        <div className="wp-meeting-meta">
          <span>{when}</span>
          <span>{dur}</span>
          {status && (
            <span className={`wp-meeting-status wp-meeting-status--${dot}`}>
              {status}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
