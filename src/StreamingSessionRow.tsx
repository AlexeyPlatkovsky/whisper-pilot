import { Icon } from "./Icon";
import type { StreamingStatusView } from "./streamingStatus";

/** One streaming-session library row: status dot, title/meta, and the
 * rename/delete actions. A spinner takes the action group's place while this
 * session is running. */
export function StreamingSessionRow({
  title,
  when,
  dur,
  status,
  selected,
  onSelect,
  onRename,
  onDelete,
}: {
  title: string;
  when: string;
  dur: string;
  status: StreamingStatusView;
  selected?: boolean;
  onSelect: () => void;
  onRename: () => void;
  onDelete: () => void;
}) {
  return (
    <li
      className={`wp-meeting-row${selected ? " is-selected" : ""}`}
      role="listitem"
      aria-label={title}
    >
      <span
        className={`wp-meeting-dot wp-tone--${status.tone} wp-status--${status.statusKey}`}
        role="img"
        aria-label={status.label}
        title={status.label}
      />
      <button
        type="button"
        className="wp-meeting-open"
        aria-label={`Open ${title}`}
        aria-current={selected ? "page" : undefined}
        onClick={onSelect}
      >
        <div className="wp-meeting-text">
          <span className="wp-meeting-title" title={title}>
            {title}
          </span>
          <div className="wp-meeting-meta">
            <span>{when}</span>
            <span>{dur}</span>
          </div>
        </div>
      </button>
      <span className="wp-meeting-actions">
        {status.spinning ? (
          <span className="wp-meeting-busy" aria-hidden="true">
            <Icon
              name="refresh-cw"
              size={13}
              className={`wp-spin wp-tone--${status.tone} wp-status--${status.statusKey}`}
            />
          </span>
        ) : (
          <>
            <button
              type="button"
              aria-label={`Rename ${title}`}
              onClick={onRename}
            >
              <Icon name="pencil" size={13} />
            </button>
            <button
              type="button"
              aria-label={`Delete ${title}`}
              onClick={onDelete}
            >
              <Icon name="trash-2" size={13} />
            </button>
          </>
        )}
      </span>
    </li>
  );
}
