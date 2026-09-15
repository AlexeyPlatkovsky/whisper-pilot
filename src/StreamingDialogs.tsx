import type { FormEvent } from "react";
import { ConfirmDialog } from "./ConfirmDialog";

export interface StreamingDialogTarget {
  id: number;
  title: string;
}

interface StreamingDialogsProps {
  renameTarget: StreamingDialogTarget | null;
  renameDraft: string;
  renameError: string | null;
  onRenameDraftChange: (value: string) => void;
  onRenameCancel: () => void;
  onSaveRename: (event: FormEvent<HTMLFormElement>) => void;
  deleteTarget: StreamingDialogTarget | null;
  onDeleteCancel: () => void;
  onDelete: () => void | Promise<void>;
  clearPending: boolean;
  hasActiveSession: boolean;
  onClearCancel: () => void;
  onClear: () => void | Promise<void>;
}

/** Dialogs are deliberately separate from live-capture state: mounting a
 * confirmation must never subscribe to, or mutate, the capture lifecycle. */
export function StreamingDialogs({
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
  hasActiveSession,
  onClearCancel,
  onClear,
}: StreamingDialogsProps) {
  return (
    <>
      {renameTarget && (
        <div className="modal-overlay">
          <form
            className="modal-panel confirm-modal"
            role="dialog"
            aria-modal="true"
            aria-label="Rename meeting"
            onSubmit={onSaveRename}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                onRenameCancel();
              }
            }}
          >
            <div className="modal-header">
              <span className="modal-title">Rename meeting</span>
            </div>
            <label htmlFor="streaming-session-label">Meeting title</label>
            <input
              id="streaming-session-label"
              type="text"
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
                Save
              </button>
            </div>
          </form>
        </div>
      )}
      {deleteTarget && (
        <ConfirmDialog
          label={`Delete ${deleteTarget.title}`}
          title={`Delete ${deleteTarget.title}?`}
          description="This permanently removes the meeting and its transcript."
          confirmLabel="Delete"
          destructive
          onCancel={onDeleteCancel}
          onConfirm={onDelete}
        />
      )}
      {clearPending && hasActiveSession && (
        <ConfirmDialog
          label="Clear meeting"
          title="Clear meeting?"
          description="The transcript, translation, MFU, and Prettify result will be permanently removed. The empty meeting will stay in the list."
          confirmLabel="Clear meeting"
          destructive
          onCancel={onClearCancel}
          onConfirm={onClear}
        />
      )}
    </>
  );
}
