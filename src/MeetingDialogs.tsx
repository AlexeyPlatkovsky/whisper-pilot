import type { FormEvent } from "react";
import type { Meeting, MeetingSummary } from "./ipc";
import { ConfirmDialog } from "./ConfirmDialog";

type RenameTarget = Pick<Meeting, "id" | "title">;

export function MeetingDialogs({
  renameTarget,
  renameDraft,
  renameError,
  onRenameDraftChange,
  onRename,
  onCloseRename,
  deleteTarget,
  onDeleteCancel,
  onDelete,
  clearPending,
  activeMeeting,
  onClearCancel,
  onClear,
  diarizationWarning,
  onDismissDiarizationWarning,
}: {
  renameTarget: RenameTarget | null;
  renameDraft: string;
  renameError: string | null;
  onRenameDraftChange: (value: string) => void;
  onRename: (event: FormEvent<HTMLFormElement>) => void;
  onCloseRename: () => void;
  deleteTarget: MeetingSummary | null;
  onDeleteCancel: () => void;
  onDelete: () => Promise<void>;
  clearPending: boolean;
  activeMeeting: Meeting | null;
  onClearCancel: () => void;
  onClear: () => Promise<void>;
  diarizationWarning: string | null;
  onDismissDiarizationWarning: () => void;
}) {
  return (
    <>
      {renameTarget && (
        <div className="modal-overlay">
          <form
            className="modal-panel confirm-modal"
            role="dialog"
            aria-modal="true"
            aria-label="Rename transcription"
            onSubmit={onRename}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                onCloseRename();
              }
            }}
          >
            <div className="modal-header">
              <span className="modal-title">Rename transcription</span>
            </div>
            <label htmlFor="meeting-label">Transcription title</label>
            <input
              id="meeting-label"
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
                onClick={onCloseRename}
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
          description="This permanently removes the transcription and its transcript."
          confirmLabel="Delete"
          destructive
          onCancel={onDeleteCancel}
          onConfirm={onDelete}
        />
      )}
      {clearPending && activeMeeting && (
        <ConfirmDialog
          label="Clear transcription"
          title="Clear transcription?"
          description="The transcript and MFU will be permanently removed. The transcription and its source file will stay available."
          confirmLabel="Clear transcription"
          destructive
          onCancel={onClearCancel}
          onConfirm={onClear}
        />
      )}
      {diarizationWarning && (
        <ConfirmDialog
          label="Speaker identification issue"
          title="Speaker identification issue"
          description={diarizationWarning}
          confirmLabel="OK"
          destructive={false}
          showCancel={false}
          onCancel={onDismissDiarizationWarning}
          onConfirm={onDismissDiarizationWarning}
        />
      )}
    </>
  );
}
