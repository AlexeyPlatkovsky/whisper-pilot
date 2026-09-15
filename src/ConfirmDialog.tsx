import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
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
  onConfirm: () => void | Promise<void>;
  onCancel: () => void;
  destructive: boolean;
  showCancel?: boolean;
  busy?: boolean;
}) {
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const confirmRef = useRef<HTMLButtonElement | null>(null);
  const confirmingRef = useRef(false);
  const [confirming, setConfirming] = useState(false);
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
      event.stopPropagation();
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

  function handleConfirm() {
    // A dialog can remain mounted while its async IPC action settles.  Do not
    // let a double click, Return, or Space issue the destructive command a
    // second time in that interval.
    if (busy || confirmingRef.current) return;
    confirmingRef.current = true;
    setConfirming(true);
    Promise.resolve()
      .then(onConfirm)
      .catch(() => {
        // The owner keeps the dialog open on failure and renders the error in
        // its workspace.  Re-enable an intentional retry in that case.
        confirmingRef.current = false;
        setConfirming(false);
      });
  }

  const disabled = busy || confirming;

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
              disabled={disabled}
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
            onClick={handleConfirm}
            disabled={disabled}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
