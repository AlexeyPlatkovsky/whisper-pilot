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

export function AiModelsSection() {
  const [models, setModels] = useState<TaskModel[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [rowState, setRowState] = useState<Record<string, RowState>>({});

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
      setRowState((prev) => ({ ...prev, [id]: { kind: "downloading", fraction } }));
    });
    return () => {
      void unlisten.then((f) => f()).catch(() => {});
    };
  }, []);

  async function handleDownload(id: string) {
    setRowState((prev) => ({ ...prev, [id]: { kind: "downloading", fraction: 0 } }));
    try {
      await downloadModel(id);
      setRowState((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      setModels(await listTaskModels());
    } catch (e) {
      setRowState((prev) => ({ ...prev, [id]: { kind: "error", message: String(e) } }));
    }
  }

  async function handleDelete(id: string) {
    try {
      await deleteModel(id);
      setModels(await listTaskModels());
    } catch (e) {
      setRowState((prev) => ({ ...prev, [id]: { kind: "error", message: String(e) } }));
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
                return (
                  <li key={m.id} className="model-row">
                    <span
                      className={`model-radio${m.downloaded ? " is-selected" : ""}`}
                      aria-hidden="true"
                    />
                    <span className="model-label">{m.label}</span>
                    <span className="model-row-spacer" />
                    {state?.kind === "downloading" && (
                      <progress
                        value={state.fraction}
                        max={1}
                        aria-label={`Downloading ${m.label}`}
                      />
                    )}
                    {state?.kind === "error" && (
                      <span role="alert">{state.message}</span>
                    )}
                    {!state && (
                      <span className="model-size">
                        {formatSize(m.size_bytes)}
                      </span>
                    )}
                    {!state && m.downloaded && (
                      <span className="model-actions">
                        <span
                          className="model-status model-status--ready"
                          aria-label="Ready"
                        >
                          <Icon name="check" size={16} />
                        </span>
                        <button
                          className="model-icon-btn"
                          aria-label={`Delete ${m.label}`}
                          title={`Delete ${m.label}`}
                          onClick={() => handleDelete(m.id)}
                        >
                          <Icon name="trash-2" size={16} />
                        </button>
                      </span>
                    )}
                    {!state && !m.downloaded && (
                      <span className="model-actions">
                        <button
                          className="model-icon-btn model-icon-btn--accent"
                          aria-label={`Download ${m.label}`}
                          title={`Download ${m.label}`}
                          onClick={() => handleDownload(m.id)}
                        >
                          <Icon name="download" size={16} />
                        </button>
                        <span className="model-icon-btn model-icon-btn--disabled">
                          <Icon name="trash-2" size={16} />
                        </span>
                      </span>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
        );
      })}
    </div>
  );
}
