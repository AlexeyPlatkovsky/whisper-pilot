import { ActionIcon } from "./ActionIcon";
import { AppLogo, Icon } from "./Icon";
import { CopyButton } from "./CopyButton";
import { collapseToBubble } from "./ipc";
import { formatElapsedClock } from "./format";
import type { StreamingStatusView } from "./streamingStatus";

interface StreamingHeaderProps {
  activeId: number | null;
  activeTitle: string;
  activeBusy: boolean;
  captureHydrated: boolean;
  busy: boolean;
  isRunning: boolean;
  isStartPending: boolean;
  isStopPending: boolean;
  meetingTranscriptionActive: boolean;
  sidebarOpen: boolean;
  elapsed: number;
  widget: StreamingStatusView;
  windowsCount: number;
  exportText: string;
  hasText: boolean;
  craftDisabled: boolean;
  canClear: boolean;
  onToggleSidebar: () => void;
  onCreate: () => void;
  onOpenSettings: () => void;
  onRename: () => void;
  onDelete: () => void;
  onStart: () => void;
  onStop: () => void;
  onCraft: () => void;
  onExport: () => void;
  onClear: () => void;
  onError: (error: string | null) => void;
}

/** Header actions are a pure projection of controller state. */
export function StreamingHeader({
  activeId,
  activeTitle,
  activeBusy,
  captureHydrated,
  busy,
  isRunning,
  isStartPending,
  isStopPending,
  meetingTranscriptionActive,
  sidebarOpen,
  elapsed,
  widget,
  windowsCount,
  exportText,
  hasText,
  craftDisabled,
  canClear,
  onToggleSidebar,
  onCreate,
  onOpenSettings,
  onRename,
  onDelete,
  onStart,
  onStop,
  onCraft,
  onExport,
  onClear,
  onError,
}: StreamingHeaderProps) {
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
            onClick={() => void collapseToBubble()}
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
              aria-label="New meeting"
              title="New meeting"
              onClick={onCreate}
              disabled={!captureHydrated || busy || isRunning}
            >
              <Icon name="plus" size={18} />
            </button>
            <span className="wp-sep" />
            <button
              type="button"
              className="wp-icon-btn"
              aria-label="Settings"
              onClick={onOpenSettings}
              disabled={!captureHydrated}
            >
              <Icon name="settings" size={18} />
            </button>
          </div>
        </div>
        <div className="wp-title-group">
          <h1 className="wp-title">{activeTitle}</h1>
          <button
            type="button"
            className="wp-icon-btn wp-icon-btn--ghost"
            aria-label="Rename meeting"
            onClick={onRename}
            disabled={activeId === null || activeBusy}
          >
            <Icon name="pencil" size={14} />
          </button>
          <button
            type="button"
            className="wp-icon-btn wp-icon-btn--ghost"
            aria-label="Delete meeting"
            onClick={onDelete}
            disabled={activeId === null || activeBusy}
          >
            <Icon name="trash-2" size={14} />
          </button>
        </div>
      </div>
      <div className="wp-header-right">
        <div className="wp-status" role="status">
          <Icon
            name={widget.icon}
            size={14}
            className={
              widget.spinning
                ? `wp-spin wp-tone--${widget.tone} wp-status--${widget.statusKey}`
                : `wp-tone--${widget.tone} wp-status--${widget.statusKey}`
            }
          />
          <span
            className={`wp-status-label wp-tone--${widget.tone} wp-status--${widget.statusKey}`}
          >
            {widget.label}
          </span>
          {widget.showTimer && (
            <span className="wp-status-timer" aria-hidden="true">
              {formatElapsedClock(elapsed)}
            </span>
          )}
        </div>
        <div className="wp-action-group">
          <ActionIcon
            icon="play"
            label={
              activeId !== null && !isRunning && windowsCount > 0
                ? "Resume"
                : "Start"
            }
            onClick={onStart}
            disabled={
              meetingTranscriptionActive ||
              !captureHydrated ||
              busy ||
              isRunning ||
              isStartPending
            }
          />
          <span className="wp-sep" />
          <ActionIcon
            icon="square"
            label="Stop"
            onClick={onStop}
            disabled={!captureHydrated || !isRunning || isStopPending}
          />
          <span className="wp-sep" />
          <ActionIcon
            icon="sparkles"
            label="Craft MFU"
            accent
            onClick={onCraft}
            disabled={craftDisabled}
          />
          <span className="wp-sep" />
          <CopyButton
            text={exportText}
            resetKey={activeId}
            onError={onError}
            onCopied={() => onError(null)}
            disabled={!hasText}
          />
          <span className="wp-sep" />
          <ActionIcon
            icon="download"
            label="Export as Markdown"
            onClick={onExport}
            disabled={!hasText}
          />
          <span className="wp-sep" />
          <ActionIcon
            icon="eraser"
            label="Clear meeting"
            onClick={onClear}
            disabled={!canClear}
          />
        </div>
      </div>
    </header>
  );
}
