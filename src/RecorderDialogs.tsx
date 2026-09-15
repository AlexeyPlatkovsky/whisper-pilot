import type { FormEvent } from "react";
import { ConfirmDialog } from "./ConfirmDialog";
import type { RecorderSession } from "./ipc";

export interface RecorderDialogTarget {
  id: number;
  title: string;
}

interface RecorderDialogsProps {
  renameTarget: RecorderDialogTarget | null;
  renameDraft: string;
  renameError: string | null;
  onRenameDraftChange: (value: string) => void;
  onRenameCancel: () => void;
  onSaveRename: (event: FormEvent) => void;
  deleteTarget: RecorderDialogTarget | null;
  onDeleteCancel: () => void;
  onDelete: (target: RecorderDialogTarget) => void;
  clearPending: boolean;
  active: RecorderSession | null;
  onClearCancel: () => void;
  onClear: () => void;
}

export function RecorderDialogs({
  renameTarget,
  renameDraft,
  renameError,
  onRenameDraftChange,
  onRenameCancel,
  onSaveRename,
  deleteTarget,
  onDeleteCancel,
  onDelete,
  clearPending,
  active,
  onClearCancel,
  onClear,
}: RecorderDialogsProps) {
  return (
    <>
      {renameTarget && (
        <div className="modal-overlay">
          <form
            className="modal-panel confirm-modal"
            role="dialog"
            aria-modal="true"
            aria-label="Rename recording"
            onSubmit={onSaveRename}
            onKeyDown={(event) => {
              if (event.key === "Escape") onRenameCancel();
            }}
          >
            <div className="modal-header">
              <span className="modal-title">Rename recording</span>
            </div>
            <label htmlFor="recorder-title">Recording title</label>
            <input
              id="recorder-title"
              type="text"
              aria-label="Recorder title"
              value={renameDraft}
              autoFocus
              onFocus={(event) => event.currentTarget.select()}
              onChange={(event) => onRenameDraftChange(event.target.value)}
              aria-invalid={renameError ? true : undefined}
            />
            {renameError && <p role="alert">{renameError}</p>}
            <div className="confirm-actions">
              <button
                type="button"
                className="modal-button"
                onClick={onRenameCancel}
              >
                Cancel
              </button>
              <button
                type="submit"
                className="modal-button modal-button--primary"
              >
                Save rename
              </button>
            </div>
          </form>
        </div>
      )}
      {deleteTarget && (
        <ConfirmDialog
          label={`Delete ${deleteTarget.title}`}
          title={`Delete ${deleteTarget.title}?`}
          description="This permanently removes the recording, its transcript, and its app-owned audio."
          confirmLabel="Delete"
          destructive
          onCancel={onDeleteCancel}
          onConfirm={() => onDelete(deleteTarget)}
        />
      )}
      {clearPending && active && (
        <ConfirmDialog
          label="Clear recording"
          title="Clear recording?"
          description="Audio and all transcript text will be permanently removed. The empty recording will stay in the list."
          confirmLabel="Clear recording"
          destructive
          onCancel={onClearCancel}
          onConfirm={onClear}
        />
      )}
    </>
  );
}
