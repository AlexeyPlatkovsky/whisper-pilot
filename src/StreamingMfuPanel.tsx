import type { StreamingMfu } from "./ipc";
import { Icon } from "./Icon";

/** Presentation-only MFU panel shared by stopped and live meetings. */
export function StreamingMfuPanel({ mfu }: { mfu: StreamingMfu | null }) {
  return (
    <aside className="wp-mfu">
      {mfu ? (
        <div className="wp-mfu-content">
          {mfu.summary && <MfuSection title="Summary" text={mfu.summary} />}
          {mfu.decisions && (
            <MfuSection title="Decisions" text={mfu.decisions} />
          )}
          {mfu.action_items && (
            <MfuSection title="Action Items" text={mfu.action_items} />
          )}
          {mfu.open_questions && (
            <MfuSection title="Open Questions" text={mfu.open_questions} />
          )}
          {mfu.participants && (
            <MfuSection title="Participants" text={mfu.participants} />
          )}
        </div>
      ) : (
        <div className="wp-mfu-placeholder">
          <div className="wp-mfu-icon-frame">
            <Icon name="sparkles" size={32} />
          </div>
          <p className="wp-mfu-title">Run MFU Craft</p>
          <p className="wp-mfu-subtitle">
            Generate summary, decisions, action items, and open questions.
          </p>
        </div>
      )}
    </aside>
  );
}

function MfuSection({ title, text }: { title: string; text: string }) {
  return (
    <section className="wp-mfu-section">
      <h3 className="wp-mfu-heading">{title}</h3>
      <p className="wp-mfu-text">{text}</p>
    </section>
  );
}
