import type { RecorderSessionSummary } from "./ipc";
import { Icon } from "./Icon";
import { ModeToggle } from "./ModeToggle";
import { StreamingSessionRow } from "./StreamingSessionRow";
import type { StreamingStatusView } from "./streamingStatus";
import { formatDuration } from "./format";

export function RecorderSidebar({
  sessions,
  filteredSessions,
  activeId,
  search,
  onSearchChange,
  onSelectMeeting,
  onSelectStreaming,
  onOpenSession,
  onRename,
  onDelete,
  statusFor,
}: {
  sessions: RecorderSessionSummary[];
  filteredSessions: RecorderSessionSummary[];
  activeId: number | null;
  search: string;
  onSearchChange: (value: string) => void;
  onSelectMeeting: () => void;
  onSelectStreaming: () => void;
  onOpenSession: (id: number) => void;
  onRename: (id: number, title: string) => void;
  onDelete: (id: number, title: string) => void;
  statusFor: (session: RecorderSessionSummary) => StreamingStatusView;
}) {
  return (
    <aside className="wp-sidebar">
      <ModeToggle
        mode="recorder"
        onSelectMeeting={onSelectMeeting}
        onSelectStreaming={onSelectStreaming}
        onSelectRecorder={() => {}}
      />
      <div className="wp-search">
        <Icon name="search" size={16} />
        <input
          type="search"
          className="wp-search-input"
          placeholder="Search recordings..."
          aria-label="Search recordings"
          value={search}
          onChange={(event) => onSearchChange(event.target.value)}
        />
      </div>
      {sessions.length === 0 ? (
        <p className="wp-info-muted">No recordings yet</p>
      ) : filteredSessions.length === 0 ? (
        <p className="wp-info-muted">No matches</p>
      ) : (
        <ul className="wp-meeting-list" role="list">
          {filteredSessions.map((session) => (
            <StreamingSessionRow
              key={session.id}
              title={session.title}
              when={new Date(session.created_at_ms).toLocaleDateString()}
              dur={formatDuration(session.duration_ms)}
              status={statusFor(session)}
              selected={session.id === activeId}
              onSelect={() => onOpenSession(session.id)}
              onRename={() => onRename(session.id, session.title)}
              onDelete={() => onDelete(session.id, session.title)}
            />
          ))}
        </ul>
      )}
    </aside>
  );
}
