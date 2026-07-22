import { useEffect, useState } from "react";
import {
  deleteModel,
  downloadModel,
  listTaskModels,
  onModelDownloadProgress,
  type TaskModel,
} from "./ipc";

type RowState =
  | { kind: "downloading"; fraction: number }
  | { kind: "error"; message: string };

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

  return (
    <ul className="model-list">
      {models.map((m) => {
        const state = rowState[m.id];
        return (
          <li key={m.id} className="model-row">
            <span className="model-label">{m.label}</span>
            {state?.kind === "downloading" && (
              <progress
                value={state.fraction}
                max={1}
                aria-label={`Downloading ${m.label}`}
              />
            )}
            {state?.kind === "error" && <span role="alert">{state.message}</span>}
            {!state && m.downloaded && (
              <span className="model-actions">
                <span className="model-status">Ready</span>
                <button onClick={() => handleDelete(m.id)}>
                  {`Delete ${m.label}`}
                </button>
              </span>
            )}
            {!state && !m.downloaded && (
              <button onClick={() => handleDownload(m.id)}>
                {`Download ${m.label}`}
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
}
