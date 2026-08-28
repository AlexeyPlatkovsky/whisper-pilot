/** A view-only pill switch, shared by the Meeting and Streaming transcript
 * headers (WP-96) so their MFU-panel toggle can't drift in markup, styling,
 * or accessibility behavior — a native `<button>` with `role="switch"`
 * keeps keyboard activation for free (see
 * `.claude/conventions/react-tauri/accessibility.md`). Enabled unless a
 * caller opts in with `disabled`; `disabledReason` then replaces the label
 * as the button's `title` so the reason surfaces without needing color
 * alone. */
export function ToggleSwitch({
  checked,
  onChange,
  label,
  disabled,
  disabledReason,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  disabled?: boolean;
  disabledReason?: string;
}) {
  const title = disabled && disabledReason ? disabledReason : label;
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      title={title}
      className={`wp-toggle-switch${checked ? " wp-toggle-switch--on" : ""}`}
      onClick={() => onChange(!checked)}
      disabled={disabled}
    >
      <span className="wp-toggle-switch-knob" />
    </button>
  );
}
