import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  type ReactNode,
} from "react";

export function ConfirmDialog({
  label,
  title,
  description,
  confirmLabel,
  onConfirm,
  onCancel,
  destructive,
  showCancel = true,
  busy = false,
}: {
  label: string;
  title: string;
  description: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  destructive: boolean;
  showCancel?: boolean;
  busy?: boolean;
}) {
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const confirmRef = useRef<HTMLButtonElement | null>(null);
  const descriptionId = useId();
  const openerRef = useRef<HTMLElement | null>(
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null,
  );

  useEffect(() => {
    return () => openerRef.current?.focus();
  }, []);

  useLayoutEffect(() => {
    confirmRef.current?.focus();
  }, []);

  function handleKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
      return;
    }
    if (event.key !== "Tab") return;
    const controls = Array.from(
      dialogRef.current?.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ) ?? [],
    );
    if (controls.length === 0) return;
    const first = controls[0];
    const last = controls[controls.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div className="modal-overlay">
      <div
        ref={dialogRef}
        className="modal-panel confirm-modal"
        role="alertdialog"
        aria-modal="true"
        aria-label={label}
        aria-describedby={descriptionId}
        onKeyDown={handleKeyDown}
      >
        <div className="modal-header">
          <span className="modal-title">{title}</span>
        </div>
        <p id={descriptionId} className="confirm-warning">
          {description}
        </p>
        <div className="confirm-actions">
          {showCancel && (
            <button
              type="button"
              className="modal-button"
              onClick={onCancel}
              disabled={busy}
            >
              Cancel
            </button>
          )}
          <button
            ref={confirmRef}
            type="button"
            className={`modal-button ${
              destructive ? "modal-button--danger" : "modal-button--primary"
            }`}
            onClick={onConfirm}
            disabled={busy}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
