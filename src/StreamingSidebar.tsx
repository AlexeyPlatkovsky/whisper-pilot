import type { StreamingSessionSummary } from "./ipc";
import { Icon } from "./Icon";
import { ModeToggle } from "./ModeToggle";
import { StreamingSessionRow } from "./StreamingSessionRow";
import { formatClockTime } from "./streamingText";
import { resolveStreamingRowStatus } from "./streamingStatus";

interface StreamingSidebarProps {
  sessions: StreamingSessionSummary[];
  filteredSessions: StreamingSessionSummary[];
  sessionSearch: string;
  activeId: number | null;
  liveCaptureSessionId: number | null;
  isRunning: boolean;
  liveCaptureWidget: ReturnType<typeof resolveStreamingRowStatus>;
  activeWidget: ReturnType<typeof resolveStreamingRowStatus>;
  onSearchChange: (value: string) => void;
  onOpen: (id: number) => void;
  onRename: (id: number, title: string) => void;
  onDelete: (id: number, title: string) => void;
  onSelectMeeting: () => void;
  onSelectRecorder?: () => void;
}

/** Library navigation does not own capture state; it only projects it. */
export function StreamingSidebar({
  sessions,
  filteredSessions,
  sessionSearch,
  activeId,
  liveCaptureSessionId,
  isRunning,
  liveCaptureWidget,
  activeWidget,
  onSearchChange,
  onOpen,
  onRename,
  onDelete,
  onSelectMeeting,
  onSelectRecorder,
}: StreamingSidebarProps) {
  return (
    <aside className="wp-sidebar">
      <ModeToggle
        mode="streaming"
        onSelectMeeting={onSelectMeeting}
        onSelectStreaming={() => {}}
        onSelectRecorder={onSelectRecorder}
      />
      <div className="wp-search">
        <Icon name="search" size={16} />
        <input
          type="search"
          className="wp-search-input"
          placeholder="Search meetings..."
          aria-label="Search meetings"
          value={sessionSearch}
          onChange={(event) => onSearchChange(event.target.value)}
        />
      </div>
      {sessions.length === 0 ? (
        <p className="wp-info-muted">No meetings yet</p>
      ) : filteredSessions.length === 0 ? (
        <p className="wp-info-muted">No matches</p>
      ) : (
        <ul className="wp-meeting-list" role="list">
          {filteredSessions.map((session) => (
            <StreamingSessionRow
              key={session.id}
              title={session.title}
              when={new Date(session.created_at_ms).toLocaleDateString()}
              dur={formatClockTime(Math.max(0, session.duration_ms ?? 0))}
              status={
                isRunning && session.id === liveCaptureSessionId
                  ? liveCaptureWidget
                  : session.id === activeId
                    ? activeWidget
                    : resolveStreamingRowStatus(session.status)
              }
              selected={activeId === session.id}
              onSelect={() => onOpen(session.id)}
              onRename={() => onRename(session.id, session.title)}
              onDelete={() => onDelete(session.id, session.title)}
            />
          ))}
        </ul>
      )}
    </aside>
  );
}
