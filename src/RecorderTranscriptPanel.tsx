import type { RefObject, UIEvent } from "react";
import type { RecorderSegment, RecorderSession } from "./ipc";
import { Icon } from "./Icon";
import type { StablePartial } from "./partialStability";

type TranscriptParagraph = Array<RecorderSegment & { outcome_ok: boolean }>;

interface RecorderTranscriptPanelProps {
  active: RecorderSession | null;
  transcriptParagraphs: TranscriptParagraph[];
  partial: string;
  partialDisplay: StablePartial;
  effectiveError: string | null;
  ownsCapture: boolean;
  polishBusy: boolean;
  editingSegmentId: number | null;
  transcriptScrollRef: RefObject<HTMLDivElement | null>;
  onScroll: (event: UIEvent<HTMLDivElement>) => void;
  onRestoreOriginal: () => void;
  onEditingChange: (segmentId: number | null) => void;
  onCommitEdit: (segment: RecorderSegment, text: string) => void;
  onEditChange: (segmentId: number, text: string) => void;
  onRecover: () => void;
  onRetryDelete: () => void;
}

export function RecorderTranscriptPanel({
  active,
  transcriptParagraphs,
  partial,
  partialDisplay,
  effectiveError,
  ownsCapture,
  polishBusy,
  editingSegmentId,
  transcriptScrollRef,
  onScroll,
  onRestoreOriginal,
  onEditingChange,
  onCommitEdit,
  onEditChange,
  onRecover,
  onRetryDelete,
}: RecorderTranscriptPanelProps) {
  return (
    <section className="wp-workspace">
      <div className="wp-transcript-panel">
        <div className="wp-transcript-header">
          <div className="wp-transcript-title-group">
            <h2 className="wp-transcript-title">Transcript</h2>
            {active && (
              <span className="wp-transcript-meta">
                {active.segments.length} segments · Editable
              </span>
            )}
          </div>
          {active?.polished_text && (
            <button
              type="button"
              className="wp-icon-btn wp-icon-btn--ghost"
              aria-label="Restore original transcript"
              title="Restore original transcript"
              onClick={onRestoreOriginal}
              disabled={polishBusy}
            >
              <Icon name="rotate-ccw" size={15} />
            </button>
          )}
        </div>
        <div className="wp-separator" />
        <div
          ref={transcriptScrollRef}
          className="wp-transcript-content wp-transcript-content--inset recorder-transcript"
          aria-label="Recorder transcript"
          onScroll={onScroll}
        >
          {effectiveError && (
            <p className="wp-error" role="alert">
              {effectiveError}
            </p>
          )}
          {!active ? (
            <div className="wp-empty-state">
              <Icon name="mic" size={28} />
              <p>Start a voice recording or open a saved recording.</p>
            </div>
          ) : active.polished_text ? (
            <p className="streaming-transcript-text">{active.polished_text}</p>
          ) : active.segments.length === 0 && !partial ? (
            <div className="wp-empty-state">
              <Icon name="messages-square" size={28} />
              <p>The transcript will appear here while you dictate.</p>
            </div>
          ) : (
            <>
              <div className="streaming-transcript-text">
                {transcriptParagraphs.map((paragraph) => (
                  <p key={paragraph[0].id} className="streaming-paragraph">
                    {paragraph.map((segment) => {
                      const isEditing = editingSegmentId === segment.id;
                      return (
                        <span key={segment.id}>
                          <span
                            className="streaming-window recorder-segment-text"
                            role="textbox"
                            aria-label={`Transcript segment ${segment.id}`}
                            aria-readonly={!isEditing}
                            tabIndex={0}
                            contentEditable={isEditing}
                            suppressContentEditableWarning
                            spellCheck
                            title={`${Math.floor(segment.start_sample / active.sample_rate)}–${Math.ceil(segment.end_sample / active.sample_rate)}s (${segment.language})`}
                            onClick={() => {
                              const selection = window.getSelection();
                              if (
                                !isEditing &&
                                (!selection || selection.isCollapsed)
                              ) {
                                onEditingChange(segment.id);
                              }
                            }}
                            onKeyDown={(event) => {
                              if (!isEditing && event.key === "Enter") {
                                event.preventDefault();
                                onEditingChange(segment.id);
                              }
                            }}
                            onBlur={(event) => {
                              if (!isEditing) return;
                              onEditingChange(null);
                              onCommitEdit(
                                segment,
                                event.currentTarget.textContent ?? "",
                              );
                            }}
                            onInput={(event) =>
                              onEditChange(
                                segment.id,
                                event.currentTarget.textContent ?? "",
                              )
                            }
                          >
                            {segment.text}
                          </span>{" "}
                        </span>
                      );
                    })}
                  </p>
                ))}
              </div>
              {partial && (
                <p className="wp-streaming-partial" role="status">
                  {partialDisplay.stable && (
                    <span className="wp-streaming-partial-stable">
                      {partialDisplay.stable}{" "}
                    </span>
                  )}
                  {partialDisplay.unstable && (
                    <em className="wp-streaming-partial-unstable">
                      {partialDisplay.unstable}
                    </em>
                  )}
                </p>
              )}
            </>
          )}
          {ownsCapture && (
            <div className="wp-streaming-autoscroll-tail" aria-hidden="true" />
          )}
          {active?.status === "recoverable" && (
            <button type="button" onClick={onRecover}>
              Recover
            </button>
          )}
          {active?.status === "delete_failed" && (
            <button type="button" onClick={onRetryDelete}>
              Retry delete
            </button>
          )}
        </div>
      </div>
    </section>
  );
}
