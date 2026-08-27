/** A view-only pill switch, shared by the Meeting and Streaming transcript
 * headers (WP-90) so their MFU-panel toggle can't drift in markup, styling,
 * or accessibility behavior. A native `<button>` with `role="switch"` keeps
 * native keyboard activation (Space/Enter) for free — see
 * `.claude/conventions/react-tauri/accessibility.md`.
 *
 * Enabled unless a caller opts in with `disabled` (WP-93's Live Translation
 * switch, gated on model readiness/Prettify state) — the MFU-panel toggle
 * omits both `disabled` props and keeps its original always-enabled
 * behavior. When disabled, `disabledReason` replaces the label as the
 * button's `title` so the reason surfaces without needing color alone. */
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
