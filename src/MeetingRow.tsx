import { Icon } from "./Icon";
import type { MeetingStatusView } from "./meetingStatus";

/** One library row: status dot, title/meta, and the rename/delete actions.
 * The busy spinner replaces the action group while this meeting is running. */
export function MeetingRow({
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
  status: MeetingStatusView;
  selected?: boolean;
  onSelect: () => void;
  onRename: () => void;
  onDelete: () => void;
}) {
  const running =
    status.tone === "transcribing" ||
    status.tone === "diarizing" ||
    status.tone === "crafting";
  return (
    <li
      className={`wp-meeting-row${selected ? " is-selected" : ""}`}
      role="listitem"
      aria-label={title}
    >
      {/* The dot is the row's whole status surface: colour for a glance, the
          `title` tooltip on hover, and the same words to a screen reader. */}
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
          {/* Long names are clipped to keep the sidebar at its fixed width, so
              the tooltip is the only way left to read one in full. */}
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
        {running ? (
          // While this meeting is transcribing or diarizing, the spinner
          // takes the action group's place — renaming or deleting a running
          // meeting is not something we want to offer mid-run. It is hidden
          // from assistive tech because the dot beside it already announces
          // the current phase; exposing both would name the same status
          // twice per row.
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
