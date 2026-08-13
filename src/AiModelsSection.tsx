import { Icon } from "./Icon";
import { formatClock } from "./format";
import { useModelLibrary } from "./useModelLibrary";

const NONE_DIARIZATION_MODEL = "none";

const STAGE_LABELS: Record<string, string> = {
  downloading: "Downloading...",
  verifying: "Verifying...",
};

const SECTION_TITLES: Record<string, { title: string; subtitle: string }> = {
  transcription: {
    title: "Transcription Models",
    subtitle: "Choose the Whisper model used to transcribe your meetings.",
  },
  diarization: {
    title: "Speaker Diarization",
    subtitle: "Identify individual speakers in your recordings.",
  },
  llm: {
    title: "MFU Models",
    subtitle: "Choose the language model used to generate meeting mfu.",
  },
};

function formatSize(bytes: number): string {
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${Math.round(bytes / 1024 ** 2)} MB`;
}

export function AiModelsSection() {
  const {
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
  } = useModelLibrary();

  if (loadError) return <p role="alert">{loadError}</p>;
  if (!models) return <p>Loading…</p>;

  const groups = new Map<string, typeof models>();
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
            <ul
              className="model-list"
              role="radiogroup"
              aria-label={heading ? heading.title : task}
            >
              {task === "diarization" && (
                <li className="model-row">
                  <input
                    type="radio"
                    name="diarization-model"
                    aria-label="None"
                    checked={diarizationModel === NONE_DIARIZATION_MODEL}
                    onChange={() =>
                      handleSelectDiarizationModel(NONE_DIARIZATION_MODEL)
                    }
                  />
                  <span className="model-label">None</span>
                  <span className="model-row-spacer" />
                </li>
              )}
              {rows.map((m) => {
                const state = rowState[m.id];
                const isDownloading = state?.kind === "downloading";
                const variantValue =
                  task === "diarization" ? m.id.slice(`${task}-`.length) : null;
                const isLlm = task === "llm";
                return (
                  <li key={m.id} className="model-row">
                    {variantValue !== null ? (
                      <input
                        type="radio"
                        name="diarization-model"
                        aria-label={m.label}
                        checked={diarizationModel === variantValue}
                        disabled={!m.downloaded}
                        onChange={() =>
                          handleSelectDiarizationModel(variantValue)
                        }
                      />
                    ) : isLlm ? (
                      <input
                        type="radio"
                        name="llm-model"
                        aria-label={m.label}
                        checked={llmModel === m.id}
                        disabled={!m.downloaded}
                        onChange={() => handleSelectLlmModel(m.id)}
                      />
                    ) : (
                      // A task with no selectable variants still shows the
                      // model in use as a checked option, so "selected" looks
                      // the same in every section. Nothing to switch to, hence
                      // no handler; disabled until it is on disk.
                      <input
                        type="radio"
                        name={`${task}-model`}
                        aria-label={m.label}
                        checked={m.downloaded}
                        disabled={!m.downloaded}
                        readOnly
                      />
                    )}
                    <span className="model-label">
                      {m.label}
                      {m.recommended && (
                        <span className="model-badge">Recommended</span>
                      )}
                    </span>
                    <span className="model-row-spacer" />
                    {state?.kind === "error" && (
                      <span className="model-row-error" role="alert">
                        {state.message}
                      </span>
                    )}
                    {/* Closing the modal dismisses only the modal, so the row
                        has to say what its spinner is still waiting for. */}
                    {isDownloading && (
                      <span className="model-row-status">
                        {state.stage === "downloading"
                          ? `${Math.round(state.fraction * 100)}%`
                          : STAGE_LABELS.verifying}
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
            {task === "diarization" && diarizationSelectError && (
              <p className="model-row-error" role="alert">
                {diarizationSelectError}
              </p>
            )}
            {task === "llm" && llmSelectError && (
              <p className="model-row-error" role="alert">
                {llmSelectError}
              </p>
            )}
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
                {STAGE_LABELS[downloadState.stage]}
              </span>
              <span className="modal-timer">{formatClock(elapsed)}</span>
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
              <button type="button" onClick={() => setConfirmDeleteId(null)}>
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
