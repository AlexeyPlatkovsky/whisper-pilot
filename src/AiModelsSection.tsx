import { useEffect, useState } from "react";
import {
  deleteModel,
  downloadModel,
  listTaskModels,
  onModelDownloadProgress,
  type TaskModel,
} from "./ipc";
import { Icon } from "./Icon";

type RowState =
  | { kind: "downloading"; fraction: number }
  | { kind: "error"; message: string };

const SECTION_TITLES: Record<string, { title: string; subtitle: string }> = {
  transcription: {
    title: "Transcription Models",
    subtitle: "Choose the Whisper model used to transcribe your meetings.",
  },
  diarization: {
    title: "Speaker Diarization",
    subtitle: "Identify individual speakers in your recordings.",
  },
};

function formatSize(bytes: number): string {
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${Math.round(bytes / 1024 ** 2)} MB`;
}

function formatElapsed(totalSeconds: number): string {
  const m = Math.floor(totalSeconds / 60);
  const s = totalSeconds % 60;
  return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
}

export function AiModelsSection() {
  const [models, setModels] = useState<TaskModel[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [rowState, setRowState] = useState<Record<string, RowState>>({});
  const [downloadModalId, setDownloadModalId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    let cancelled = false;
    listTaskModels()
      .then((list) => {
        if (!cancelled) setModels(list);
      })
      .catch((e) => {
        if (!cancelled) setLoadError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const unlisten = onModelDownloadProgress(({ id, fraction }) => {
      setRowState((prev) => ({
        ...prev,
        [id]: { kind: "downloading", fraction },
      }));
    });
    return () => {
      void unlisten.then((f) => f()).catch(() => {});
    };
  }, []);

  // Modal auto-closes once the download it tracks is no longer in progress
  // (finished or failed) — the row status updates the same way either way.
  useEffect(() => {
    if (downloadModalId && rowState[downloadModalId]?.kind !== "downloading") {
      setDownloadModalId(null);
    }
  }, [downloadModalId, rowState]);

  useEffect(() => {
    if (!downloadModalId) return;
    setElapsed(0);
    const id = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(id);
  }, [downloadModalId]);

  async function handleDownload(id: string) {
    setDownloadModalId(id);
    setRowState((prev) => ({
      ...prev,
      [id]: { kind: "downloading", fraction: 0 },
    }));
    try {
      await downloadModel(id);
      setRowState((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      setModels(await listTaskModels());
    } catch (e) {
      setRowState((prev) => ({
        ...prev,
        [id]: { kind: "error", message: String(e) },
      }));
    }
  }

  async function handleDelete(id: string) {
    setConfirmDeleteId(null);
    try {
      await deleteModel(id);
      setModels(await listTaskModels());
    } catch (e) {
      setRowState((prev) => ({
        ...prev,
        [id]: { kind: "error", message: String(e) },
      }));
    }
  }

  if (loadError) return <p role="alert">{loadError}</p>;
  if (!models) return <p>Loading…</p>;

  const groups = new Map<string, TaskModel[]>();
  for (const m of models) {
    const list = groups.get(m.task) ?? [];
    list.push(m);
    groups.set(m.task, list);
  }

  const downloadTarget = downloadModalId
    ? models.find((m) => m.id === downloadModalId)
    : undefined;
  const downloadState =
    downloadModalId && rowState[downloadModalId]?.kind === "downloading"
      ? rowState[downloadModalId]
      : undefined;

  const deleteTarget = confirmDeleteId
    ? models.find((m) => m.id === confirmDeleteId)
    : undefined;

  return (
    <div className="model-sections">
      {[...groups.entries()].map(([task, rows]) => {
        const heading = SECTION_TITLES[task];
        return (
          <section className="model-section" key={task}>
            {heading && (
              <div className="section-header">
                <h4 className="section-title">{heading.title}</h4>
                <p className="section-subtitle">{heading.subtitle}</p>
              </div>
            )}
            <ul className="model-list">
              {rows.map((m) => {
                const state = rowState[m.id];
                const isDownloading = state?.kind === "downloading";
                return (
                  <li key={m.id} className="model-row">
                    <span
                      className={`model-radio${m.downloaded ? " is-selected" : ""}`}
                      aria-hidden="true"
                    />
                    <span className="model-label">{m.label}</span>
                    <span className="model-row-spacer" />
                    {state?.kind === "error" && (
                      <span className="model-row-error" role="alert">
                        {state.message}
                      </span>
                    )}
                    <span className="model-size">
                      {formatSize(m.size_bytes)}
                    </span>
                    <span className="model-actions">
                      <span className="model-icon-slot">
                        {m.downloaded ? (
                          <span className="model-status model-status--ready">
                            <Icon name="check" size={16} />
                            <span className="sr-only">Ready</span>
                          </span>
                        ) : (
                          <button
                            type="button"
                            className="model-icon-btn model-icon-btn--accent"
                            aria-label={`Download ${m.label}`}
                            title={`Download ${m.label}`}
                            disabled={isDownloading}
                            onClick={() => handleDownload(m.id)}
                          >
                            <Icon
                              name={isDownloading ? "refresh-cw" : "download"}
                              size={16}
                              className={isDownloading ? "wp-spin" : undefined}
                            />
                          </button>
                        )}
                      </span>
                      <button
                        type="button"
                        className="model-icon-btn"
                        aria-label={`Delete ${m.label}`}
                        title={`Delete ${m.label}`}
                        disabled={!m.downloaded}
                        onClick={() => setConfirmDeleteId(m.id)}
                      >
                        <Icon name="trash-2" size={16} />
                      </button>
                    </span>
                  </li>
                );
              })}
            </ul>
          </section>
        );
      })}

      {downloadTarget && downloadState && (
        <div className="modal-overlay">
          <div
            className="modal-panel download-modal"
            role="dialog"
            aria-modal="true"
            aria-label={`Download ${downloadTarget.label}`}
          >
            <div className="modal-header">
              <span className="modal-title">
                Download {downloadTarget.label}
              </span>
              <button
                type="button"
                className="model-icon-btn"
                aria-label="Close"
                title="Close"
                onClick={() => setDownloadModalId(null)}
              >
                <Icon name="x" size={18} />
              </button>
            </div>
            <progress
              className="modal-progress"
              value={downloadState.fraction}
              max={1}
              aria-label={`Downloading ${downloadTarget.label}`}
            />
            <div className="modal-bottom">
              <span className="modal-status">
                <Icon name="refresh-cw" size={14} className="wp-spin" />
                Downloading...
              </span>
              <span className="modal-timer">{formatElapsed(elapsed)}</span>
            </div>
          </div>
        </div>
      )}

      {deleteTarget && (
        <div className="modal-overlay">
          <div
            className="modal-panel confirm-modal"
            role="alertdialog"
            aria-modal="true"
            aria-label={`Delete ${deleteTarget.label}`}
          >
            <div className="modal-header">
              <span className="modal-title">Delete {deleteTarget.label}?</span>
            </div>
            <p className="confirm-warning">
              This removes the downloaded model from disk. You can download it
              again later.
            </p>
            <div className="confirm-actions">
              <button
                type="button"
                onClick={() => setConfirmDeleteId(null)}
              >
                Cancel
              </button>
              <button
                type="button"
                className="danger"
                onClick={() => handleDelete(deleteTarget.id)}
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
