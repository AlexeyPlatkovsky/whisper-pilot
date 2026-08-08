import { Icon, type IconName } from "./Icon";

/** A single icon button for a `.wp-action-group` row (the header's Main
 * Actions icon strip), shared by the Meeting and Streaming windows so their
 * action rows can't drift in markup or styling. */
export function ActionIcon({
  icon,
  label,
  accent,
  disabled,
  onClick,
}: {
  icon: IconName;
  label: string;
  accent?: boolean;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      className={`wp-icon-btn${accent ? " wp-icon-btn--accent" : ""}`}
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
    >
      <Icon name={icon} size={17} />
    </button>
  );
}
