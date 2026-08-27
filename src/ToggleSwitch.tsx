/** A view-only pill switch, shared by the Meeting and Streaming transcript
 * headers (WP-90) so their MFU-panel toggle can't drift in markup, styling,
 * or accessibility behavior. A native `<button>` with `role="switch"` keeps
 * native keyboard activation (Space/Enter) for free — see
 * `.claude/conventions/react-tauri/accessibility.md`.
 *
 * Always enabled: this component has no `disabled` prop, so a caller cannot
 * accidentally gate it behind a busy/running state. */
export function ToggleSwitch({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      title={label}
      className={`wp-toggle-switch${checked ? " wp-toggle-switch--on" : ""}`}
      onClick={() => onChange(!checked)}
    >
      <span className="wp-toggle-switch-knob" />
    </button>
  );
}
