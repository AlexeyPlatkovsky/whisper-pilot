import { AppLogo, Icon } from "./Icon";
import type { RecorderSession } from "./ipc";
import { RecorderActionGroup } from "./RecorderActionGroup";
import type { StreamingStatusView } from "./streamingStatus";

interface RecorderHeaderProps {
  active: RecorderSession | null;
  sidebarOpen: boolean;
  captureReady: boolean;
  newRecordingDisabled: boolean;
  destructiveDisabled: boolean;
  widget: StreamingStatusView;
  elapsedLabel: string;
  transcript: string;
  rawTranscript: string;
  canStart: boolean;
  canStop: boolean;
  polishBusy: boolean;
  hasPendingSegmentEdits: boolean;
  onCollapse: () => void;
  onToggleSidebar: () => void;
  onNewRecording: () => void;
  onOpenSettings: () => void;
  onRename: (id: number, title: string) => void;
  onDelete: (id: number, title: string) => void;
  onStart: () => void;
  onStop: () => void;
  onPrettify: () => void;
  onExport: () => void;
  onClear: () => void;
  onError: (message: string | null) => void;
}

export function RecorderHeader({
  active,
  sidebarOpen,
  captureReady,
  newRecordingDisabled,
  destructiveDisabled,
  widget,
  elapsedLabel,
  transcript,
  rawTranscript,
  canStart,
  canStop,
  polishBusy,
  hasPendingSegmentEdits,
  onCollapse,
  onToggleSidebar,
  onNewRecording,
  onOpenSettings,
  onRename,
  onDelete,
  onStart,
  onStop,
  onPrettify,
  onExport,
  onClear,
  onError,
}: RecorderHeaderProps) {
  const clearDisabled =
    !active ||
    polishBusy ||
    hasPendingSegmentEdits ||
    destructiveDisabled ||
    active.is_draft === true;
  const prettifyDisabled =
    !active ||
    polishBusy ||
    hasPendingSegmentEdits ||
    destructiveDisabled ||
    !rawTranscript.trim();

  return (
    <header className="wp-header" data-tauri-drag-region="deep">
      <div className="wp-header-lead">
        <div className="wp-header-left">
          <span
            className="wp-traffic-space"
            aria-hidden="true"
            data-tauri-drag-region
          />
          <button
            type="button"
            className="wp-logo-button"
            aria-label="Collapse to floating bubble"
            onClick={onCollapse}
          >
            <AppLogo size={28} />
          </button>
          <div className="wp-action-group">
            <button
              type="button"
              className="wp-icon-btn"
              aria-label="Toggle sidebar"
              aria-pressed={sidebarOpen}
              onClick={onToggleSidebar}
            >
              <Icon name="panel-left" size={18} />
            </button>
            <span className="wp-sep" />
            <button
              type="button"
              className="wp-icon-btn"
              aria-label="New recording"
              title="New recording"
              onClick={onNewRecording}
              disabled={newRecordingDisabled}
            >
              <Icon name="plus" size={18} />
            </button>
            <span className="wp-sep" />
            <button
              type="button"
              className="wp-icon-btn"
              aria-label="Settings"
              onClick={onOpenSettings}
              disabled={!captureReady}
            >
              <Icon name="settings" size={18} />
            </button>
          </div>
        </div>

        <div className="wp-title-group">
          <h1 className="wp-title">{active?.title ?? "New recording"}</h1>
          <button
            type="button"
            className="wp-icon-btn wp-icon-btn--ghost"
            aria-label="Rename recording"
            onClick={() => active && onRename(active.id, active.title)}
            disabled={!active || destructiveDisabled}
          >
            <Icon name="pencil" size={14} />
          </button>
          <button
            type="button"
            className="wp-icon-btn wp-icon-btn--ghost"
            aria-label="Delete recording"
            onClick={() => active && onDelete(active.id, active.title)}
            disabled={!active || destructiveDisabled}
          >
            <Icon name="trash-2" size={14} />
          </button>
        </div>
      </div>

      <div className="wp-header-right">
        <div
          className={`wp-status wp-status--${widget.statusKey}`}
          role="status"
        >
          <Icon
            name={widget.icon}
            size={14}
            className={`${widget.spinning ? "wp-spin " : ""}wp-tone--${widget.tone} wp-status--${widget.statusKey}`}
          />
          <span
            className={`wp-status-label wp-tone--${widget.tone} wp-status--${widget.statusKey}`}
          >
            {widget.label}
          </span>
          {widget.showTimer && (
            <span className="wp-status-timer" aria-hidden="true">
              {elapsedLabel}
            </span>
          )}
        </div>

        <RecorderActionGroup
          transcript={transcript}
          resetKey={active?.id ?? null}
          startDisabled={!canStart}
          stopDisabled={!canStop}
          prettifyDisabled={prettifyDisabled}
          exportDisabled={!active || !transcript.trim()}
          clearDisabled={clearDisabled}
          onStart={onStart}
          onStop={onStop}
          onPrettify={onPrettify}
          onExport={onExport}
          onClear={onClear}
          onError={onError}
        />
      </div>
    </header>
  );
}
