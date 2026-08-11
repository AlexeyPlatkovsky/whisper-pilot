import { useEffect, useRef, useState } from "react";
import { ActionIcon } from "./ActionIcon";

// How long the copy confirmation (checked button state + toast) stays visible
// before both roll back to the default appearance.
const COPY_FEEDBACK_MS = 2500;

/** Shared Meeting/Streaming copy action. Success shows transient checked and
 * top-toast feedback; `resetKey` prevents it leaking to another workspace. */
export function CopyButton({
  text,
  disabled,
  resetKey,
  onError,
  onCopied,
}: {
  text: string;
  disabled?: boolean;
  resetKey: number | null;
  onError: (message: string) => void;
  onCopied?: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Mirrors `resetKey` for the async continuation in handleClick, which would
  // otherwise close over the value from when the click started.
  const resetKeyRef = useRef(resetKey);

  useEffect(() => {
    resetKeyRef.current = resetKey;
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    setCopied(false);
  }, [resetKey]);

  // A pending timer must not fire into an unmounted view.
  useEffect(
    () => () => {
      if (timerRef.current !== null) clearTimeout(timerRef.current);
    },
    [],
  );

  async function handleClick() {
    const keyAtClick = resetKeyRef.current;
    try {
      await navigator.clipboard.writeText(text);
    } catch (error) {
      // A switch during the in-flight write moved the workspace on; the
      // failure belongs to the meeting/session that issued it, not the new one.
      if (resetKeyRef.current === keyAtClick) onError(String(error));
      return;
    }
    // Same rule on success: late feedback must not paint onto a
    // meeting/session that was opened while the write was in flight.
    if (resetKeyRef.current !== keyAtClick) return;
    if (timerRef.current !== null) clearTimeout(timerRef.current);
    setCopied(true);
    onCopied?.();
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      setCopied(false);
    }, COPY_FEEDBACK_MS);
  }

  return (
    <>
      <ActionIcon
        icon={copied ? "check" : "copy"}
        label={copied ? "Copied" : "Copy transcript"}
        accent={copied}
        disabled={disabled}
        onClick={() => void handleClick()}
      />
      {copied && (
        <div
          className="wp-toast wp-toast--top"
          role="status"
          aria-atomic="true"
        >
          Copied
        </div>
      )}
    </>
  );
}
