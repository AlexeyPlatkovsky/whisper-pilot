// The AI-models section's library state and handlers: listing models, tracking
// download/delete progress, and the active model selections. Kept out of
// `AiModelsSection` so the row-rendering component stays presentation-only.

import { useEffect, useState } from "react";
import {
  deleteModel,
  downloadModel,
  getSettings,
  listTaskModels,
  onModelDownloadProgress,
  setSetting,
  type ModelDownloadStage,
  type TaskModel,
} from "./ipc";

const NONE_DIARIZATION_MODEL = "none";

export type RowState =
  | { kind: "downloading"; fraction: number; stage: ModelDownloadStage }
  | { kind: "error"; message: string };

export interface ModelLibrary {
  models: TaskModel[] | null;
  loadError: string | null;
  rowState: Record<string, RowState>;
  downloadModalId: string | null;
  confirmDeleteId: string | null;
  elapsed: number;
  diarizationModel: string | null;
  diarizationSelectError: string | null;
  llmModel: string | null;
  llmSelectError: string | null;
  setDownloadModalId: (id: string | null) => void;
  setConfirmDeleteId: (id: string | null) => void;
  handleDownload: (id: string) => Promise<void>;
  handleSelectDiarizationModel: (value: string) => Promise<void>;
  handleSelectLlmModel: (value: string) => Promise<void>;
  handleDelete: (id: string) => Promise<void>;
}

export function useModelLibrary(): ModelLibrary {
  const [models, setModels] = useState<TaskModel[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [rowState, setRowState] = useState<Record<string, RowState>>({});
  const [downloadModalId, setDownloadModalId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [diarizationModel, setDiarizationModel] = useState<string | null>(null);
  const [diarizationSelectError, setDiarizationSelectError] = useState<
    string | null
  >(null);
  const [llmModel, setLlmModel] = useState<string | null>(null);
  const [llmSelectError, setLlmSelectError] = useState<string | null>(null);

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
    let cancelled = false;
    getSettings()
      .then((s) => {
        if (!cancelled) {
          setDiarizationModel(
            s.active_model_diarization ?? NONE_DIARIZATION_MODEL,
          );
          setLlmModel(s.active_model_llm ?? null);
        }
      })
      .catch(() => {
        // Load failure here only disables the radio group's initial
        // selection; listTaskModels's own load error already surfaces to
        // the user above.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const unlisten = onModelDownloadProgress(({ id, fraction, stage }) => {
      setRowState((prev) => ({
        ...prev,
        [id]: { kind: "downloading", fraction, stage },
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
      [id]: { kind: "downloading", fraction: 0, stage: "downloading" },
    }));
    try {
      await downloadModel(id);
      setRowState((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      const updated = await listTaskModels();
      setModels(updated);

      const downloadedModel = updated.find((m) => m.id === id);
      if (downloadedModel?.task === "llm" && !llmModel) {
        await handleSelectLlmModel(id);
      }
    } catch (e) {
      setRowState((prev) => ({
        ...prev,
        [id]: { kind: "error", message: String(e) },
      }));
    }
  }

  async function handleSelectDiarizationModel(value: string) {
    const previous = diarizationModel;
    setDiarizationSelectError(null);
    setDiarizationModel(value);
    try {
      await setSetting("active_model.diarization", value);
    } catch (e) {
      setDiarizationModel(previous);
      setDiarizationSelectError(String(e));
    }
  }

  async function handleSelectLlmModel(value: string) {
    const previous = llmModel;
    setLlmSelectError(null);
    setLlmModel(value);
    try {
      await setSetting("active_model.llm", value);
    } catch (e) {
      setLlmModel(previous);
      setLlmSelectError(String(e));
    }
  }

  async function handleDelete(id: string) {
    setConfirmDeleteId(null);
    try {
      await deleteModel(id);
      setModels(await listTaskModels());
      // Deleting the active diarization variant reverts the backend
      // setting to "none"; re-read it so the radio group's selection
      // doesn't keep pointing at a model that no longer exists on disk.
      const settings = await getSettings();
      setDiarizationModel(
        settings.active_model_diarization ?? NONE_DIARIZATION_MODEL,
      );
      setLlmModel(settings.active_model_llm ?? null);
    } catch (e) {
      setRowState((prev) => ({
        ...prev,
        [id]: { kind: "error", message: String(e) },
      }));
    }
  }

  return {
    models,
    loadError,
    rowState,
    downloadModalId,
    confirmDeleteId,
    elapsed,
    diarizationModel,
    diarizationSelectError,
    llmModel,
    llmSelectError,
    setDownloadModalId,
    setConfirmDeleteId,
    handleDownload,
    handleSelectDiarizationModel,
    handleSelectLlmModel,
    handleDelete,
  };
}
