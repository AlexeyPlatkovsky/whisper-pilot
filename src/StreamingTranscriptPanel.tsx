import { Icon } from "./Icon";
import type {
  ChangeEvent,
  Dispatch,
  MutableRefObject,
  SetStateAction,
  UIEvent,
} from "react";
import { ToggleSwitch } from "./ToggleSwitch";
import { computeWordDiff } from "./diff";
import {
  formatClockTime,
  plainTranscript,
  windowText,
  windowTranslationDisplay,
} from "./streamingText";
import type {
  CloudProviderConfiguration,
  StreamingTranslationTargetLanguage,
  StreamingWindow,
} from "./ipc";
import type { TranslationEntry } from "./streamingText";

export interface StreamingTranscriptModel {
  headerLocked: boolean;
  transcriptionEngine: "local" | "cloud";
  setError: Dispatch<SetStateAction<string | null>>;
  setTranscriptionEngine: Dispatch<SetStateAction<"local" | "cloud">>;
  translationEnabled: boolean;
  handleToggleTranslation: (next: boolean) => void;
  translationDisabledReason: string | null;
  targetLanguage: StreamingTranslationTargetLanguage;
  handleTargetLanguageChange: (
    next: StreamingTranslationTargetLanguage,
  ) => void;
  pendingPrettify: { original: string; cleaned: string } | null;
  handleAcceptPrettify: () => Promise<void>;
  handleCancelPrettify: () => void;
  prettifiedText: string | null;
  handleRevertPrettify: () => Promise<void>;
  prettifyDisabledByTranslation: boolean;
  handlePrettify: () => Promise<void>;
  canPrettify: boolean;
  mfuPanelVisible: boolean;
  handleToggleMfuPanel: (next: boolean) => void;
  selectedCloudProvider:
    CloudProviderConfiguration["providers"][number] | undefined;
  transcriptScrollRef: MutableRefObject<HTMLDivElement | null>;
  translationSelectionColumn: "source" | "target" | null;
  setTranslationSelectionColumn: Dispatch<
    SetStateAction<"source" | "target" | null>
  >;
  handleTranscriptScroll: (event: UIEvent<HTMLDivElement>) => void;
  error: string | null;
  windows: StreamingWindow[];
  selectedOwnsLiveCapture: boolean;
  hasVisiblePartial: boolean;
  paragraphs: StreamingWindow[][];
  translations: Map<number, TranslationEntry>;
  handleRetryTranslation: (paragraphKey: number) => void;
  partialTranscript: { itemId: string | null; text: string } | null;
  partialTranslation: { sourceText: string; translatedText?: string } | null;
  TARGET_LANGUAGE_NAMES: Record<StreamingTranslationTargetLanguage, string>;
}

export function StreamingTranscriptPanel({
  view,
}: {
  view: StreamingTranscriptModel;
}) {
  const {
    headerLocked,
    transcriptionEngine,
    setError,
    setTranscriptionEngine,
    translationEnabled,
    handleToggleTranslation,
    translationDisabledReason,
    targetLanguage,
    handleTargetLanguageChange,
    pendingPrettify,
    handleAcceptPrettify,
    handleCancelPrettify,
    prettifiedText,
    handleRevertPrettify,
    prettifyDisabledByTranslation,
    handlePrettify,
    canPrettify,
    mfuPanelVisible,
    handleToggleMfuPanel,
    selectedCloudProvider,
    transcriptScrollRef,
    translationSelectionColumn,
    setTranslationSelectionColumn,
    handleTranscriptScroll,
    error,
    windows,
    selectedOwnsLiveCapture,
    hasVisiblePartial,
    paragraphs,
    translations,
    handleRetryTranslation,
    partialTranscript,
    partialTranslation,
    TARGET_LANGUAGE_NAMES,
  } = view;
  return (
    <div className="wp-transcript-panel wp-transcript-panel--streaming">
      <div
        className={
          headerLocked
            ? "wp-transcript-header wp-transcript-header--locked"
            : "wp-transcript-header"
        }
      >
        <div className="wp-transcript-title-group">
          <h2 className="wp-transcript-title">Live Transcript</h2>
          <div
            className="wp-ai-engine-toggle"
            aria-label="Transcription engine"
          >
            <span className="wp-ai-engine-label">AI</span>
            <button
              type="button"
              className={
                transcriptionEngine === "local"
                  ? "wp-engine-icon-button wp-engine-icon-button--active"
                  : "wp-engine-icon-button"
              }
              aria-label="Use local transcription"
              aria-pressed={transcriptionEngine === "local"}
              title="Local transcription"
              onClick={() => {
                setError(null);
                setTranscriptionEngine("local");
              }}
              disabled={headerLocked}
            >
              <Icon name="cpu" size={15} />
            </button>
            <button
              type="button"
              className={
                transcriptionEngine === "cloud"
                  ? "wp-engine-icon-button wp-engine-icon-button--active"
                  : "wp-engine-icon-button"
              }
              aria-label="Use cloud transcription"
              aria-pressed={transcriptionEngine === "cloud"}
              title="Cloud transcription"
              onClick={() => {
                setError(null);
                setTranscriptionEngine("cloud");
              }}
              disabled={headerLocked}
            >
              <Icon name="cloud" size={15} />
            </button>
          </div>
        </div>
        <div className="wp-translation-control">
          <Icon name="languages" size={15} />
          <ToggleSwitch
            checked={translationEnabled}
            onChange={handleToggleTranslation}
            label="Live Translation"
            disabled={translationDisabledReason !== null}
            disabledReason={translationDisabledReason ?? undefined}
          />
          <select
            className="wp-translation-lang-select"
            aria-label="Live Translation target language"
            value={targetLanguage}
            disabled={translationEnabled || headerLocked}
            title={
              headerLocked
                ? "Meeting controls are unavailable while capture is active."
                : translationEnabled
                  ? "Turn off Live Translation to change the target language."
                  : "Target language"
            }
            onChange={(event: ChangeEvent<HTMLSelectElement>) =>
              handleTargetLanguageChange(
                event.target.value as StreamingTranslationTargetLanguage,
              )
            }
          >
            <option value="en">English</option>
            <option value="ru">Русский</option>
          </select>
        </div>
        <div className="wp-transcript-actions">
          <Icon name="pencil" size={14} />
          <span className="wp-transcript-editable-label">Editable</span>
          <span className="wp-sep" />
          {pendingPrettify && (
            <>
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="Accept Prettify"
                title="Accept"
                onClick={() => void handleAcceptPrettify()}
                disabled={headerLocked}
              >
                <Icon name="check" size={15} className="wp-tone--finished" />
              </button>
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="Cancel Prettify"
                title="Cancel"
                onClick={handleCancelPrettify}
                disabled={headerLocked}
              >
                <Icon name="x" size={15} />
              </button>
            </>
          )}
          {!pendingPrettify && prettifiedText !== null && (
            <button
              type="button"
              className="wp-icon-btn"
              aria-label="Cancel Prettify"
              title="Revert to original transcript"
              onClick={() => void handleRevertPrettify()}
              disabled={headerLocked}
            >
              <Icon name="x" size={15} />
            </button>
          )}
          <button
            type="button"
            className="wp-icon-btn wp-icon-btn--accent"
            aria-label="Prettify transcript"
            title={
              prettifyDisabledByTranslation
                ? "Turn off Live Translation to use Prettify."
                : "Prettify transcript"
            }
            onClick={() => void handlePrettify()}
            disabled={
              canPrettify ||
              pendingPrettify !== null ||
              prettifyDisabledByTranslation
            }
          >
            <Icon name="wand-sparkles" size={15} />
          </button>
          <span className="wp-sep" />
          <span className="wp-mfu-toggle-label">MFU</span>
          <ToggleSwitch
            checked={mfuPanelVisible}
            onChange={handleToggleMfuPanel}
            label="MFU panel"
          />
        </div>
      </div>
      <div className="wp-separator" />

      {transcriptionEngine === "cloud" && (
        <div className="wp-cloud-notice" role="status">
          <Icon name="cloud-alert" size={15} />
          <span>
            Cloud transcription sends live audio to{" "}
            {selectedCloudProvider?.name ?? "your selected provider"}. Usage is
            billed to your account.
          </span>
        </div>
      )}

      <div
        ref={transcriptScrollRef}
        className="wp-transcript-content wp-transcript-content--inset"
        data-selection-column={translationSelectionColumn ?? undefined}
        onPointerDownCapture={(event) => {
          const target =
            event.target instanceof Element
              ? event.target.closest<HTMLElement>(
                  ".wp-translation-col--source, .wp-translation-col--target",
                )
              : null;
          if (!target) {
            setTranslationSelectionColumn(null);
          } else if (target.classList.contains("wp-translation-col--source")) {
            setTranslationSelectionColumn("source");
          } else {
            setTranslationSelectionColumn("target");
          }
        }}
        onScroll={handleTranscriptScroll}
      >
        {error && (
          <div className="wp-notice wp-notice--error" role="alert">
            {error}
          </div>
        )}
        {windows.length === 0 &&
        !(selectedOwnsLiveCapture && hasVisiblePartial) ? (
          <div className="wp-empty">
            <p>
              {selectedOwnsLiveCapture
                ? "Listening…"
                : "Start a meeting, or open one from the list."}
            </p>
          </div>
        ) : pendingPrettify ? (
          <div className="streaming-transcript-text">
            {computeWordDiff(
              pendingPrettify.original,
              pendingPrettify.cleaned,
            ).map((span, i) => {
              // <del>/<ins> so a screen reader announces the change,
              // not just strikethrough/color a sighted user sees.
              if (span.type === "del") {
                return (
                  <del key={i} className="diff-del">
                    {span.text}
                  </del>
                );
              }
              if (span.type === "add") {
                return (
                  <ins key={i} className="diff-add">
                    {span.text}
                  </ins>
                );
              }
              return <span key={i}>{span.text}</span>;
            })}
          </div>
        ) : prettifiedText !== null ? (
          <div className="streaming-transcript-text">{prettifiedText}</div>
        ) : translationEnabled ? (
          <div
            className="wp-translation-grid"
            data-selection-column={translationSelectionColumn ?? undefined}
          >
            <div className="wp-translation-columns wp-translation-columns--header">
              <div className="wp-translation-col wp-translation-col--source">
                <span className="wp-translation-col-label">
                  ORIGINAL · AUTO-DETECTED
                </span>
              </div>
              <div className="wp-translation-col-divider" />
              <div className="wp-translation-col wp-translation-col--target">
                <span className="wp-translation-col-label">
                  {TARGET_LANGUAGE_NAMES[targetLanguage].toUpperCase()}
                </span>
              </div>
            </div>
            {paragraphs.map((paragraph: StreamingWindow[]) => {
              const key = paragraph[0].window_index;
              const sourceText = plainTranscript(paragraph);
              const lastWindow = paragraph[paragraph.length - 1];
              // WP-103: retry stays a single paragraph-level affordance
              // — shown whenever at least one of this paragraph's
              // windows is currently failed (with its stored source
              // text still matching, i.e. not stale).
              const hasFailedWindow = paragraph.some((w: StreamingWindow) => {
                const entry = translations.get(w.window_index);
                return (
                  entry !== undefined &&
                  entry.sourceText === windowText(w) &&
                  entry.status === "failed"
                );
              });
              return (
                <div
                  key={key}
                  className="wp-translation-columns wp-translation-row"
                >
                  <div className="wp-translation-col wp-translation-col--source">
                    <div className="wp-translation-meta">
                      <span className="wp-translation-timestamp">
                        {formatClockTime(paragraph[0].start_ms)}–
                        {formatClockTime(lastWindow.end_ms)}
                      </span>
                      <span className="wp-translation-lang-tag">
                        {paragraph[0].language.toUpperCase()}
                      </span>
                    </div>
                    <p className="wp-translation-text">{sourceText}</p>
                  </div>
                  <div className="wp-translation-col-divider" />
                  <div className="wp-translation-col wp-translation-col--target">
                    <div className="wp-translation-meta">
                      <span className="wp-translation-lang-tag">
                        {targetLanguage.toUpperCase()}
                      </span>
                    </div>
                    {/* WP-103: each window renders its own slice —
                          real text once done/mirrored, an inline
                          placeholder otherwise — so a paragraph with a
                          still-in-flight trailing window shows real text
                          for its finished windows and a placeholder only
                          for that tail, not a blank/all-placeholder
                          cell. */}
                    <p className="wp-translation-text">
                      {paragraph.map((w: StreamingWindow, i: number) => {
                        const display = windowTranslationDisplay(
                          w,
                          translations.get(w.window_index),
                        );
                        return (
                          <span key={w.window_index}>
                            {display.kind === "text" ? (
                              <span
                                className={
                                  display.mirrored
                                    ? "wp-translation-text--mirrored"
                                    : undefined
                                }
                              >
                                {display.text}
                              </span>
                            ) : display.kind === "translating" ? (
                              <span className="wp-translation-translating">
                                <Icon
                                  name="loader"
                                  size={13}
                                  className="wp-spin"
                                />
                                <span>Translating…</span>
                              </span>
                            ) : display.kind === "failed" ? (
                              <span>Translation failed</span>
                            ) : display.kind === "unavailable" ? (
                              <span className="wp-translation-unavailable">
                                [unavailable]
                              </span>
                            ) : (
                              <span className="wp-translation-pending">
                                Pending…
                              </span>
                            )}
                            {i < paragraph.length - 1 ? " " : ""}
                          </span>
                        );
                      })}
                    </p>
                    {hasFailedWindow && (
                      <button
                        type="button"
                        className="wp-translation-retry"
                        onClick={() => handleRetryTranslation(key)}
                      >
                        <Icon name="rotate-ccw" size={13} />
                        Translation failed · Retry
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="streaming-transcript-text">
            {paragraphs.map((paragraph: StreamingWindow[]) => (
              <p
                key={paragraph[0].window_index}
                className="streaming-paragraph"
              >
                {paragraph.map((w: StreamingWindow) => (
                  <span
                    key={w.window_index}
                    className={
                      w.outcome_ok
                        ? "streaming-window"
                        : "streaming-window streaming-window--failed"
                    }
                    title={`${formatClockTime(w.start_ms)}–${formatClockTime(w.end_ms)} (${w.language})`}
                  >
                    {w.outcome_ok ? w.text : "[unavailable]"}{" "}
                  </span>
                ))}
              </p>
            ))}
          </div>
        )}
        {selectedOwnsLiveCapture &&
          hasVisiblePartial &&
          partialTranscript &&
          (translationEnabled ? (
            <div className="wp-translation-columns wp-translation-row wp-translation-row--partial">
              <div className="wp-translation-col wp-translation-col--source">
                <p className="wp-streaming-partial" role="status">
                  <em className="wp-streaming-partial-unstable">
                    {partialTranscript.text}
                  </em>
                </p>
              </div>
              <div className="wp-translation-col-divider" />
              <div className="wp-translation-col wp-translation-col--target">
                <p className="wp-streaming-partial wp-streaming-partial--translation">
                  {transcriptionEngine === "local" ? (
                    <em className="wp-translation-pending">
                      Waiting for final transcript…
                    </em>
                  ) : partialTranslation?.translatedText ? (
                    <em>{partialTranslation.translatedText}</em>
                  ) : (
                    <em className="wp-translation-pending">Translating…</em>
                  )}
                </p>
              </div>
            </div>
          ) : (
            <p className="wp-streaming-partial" role="status">
              <em className="wp-streaming-partial-unstable">
                {partialTranscript.text}
              </em>
            </p>
          ))}
        {selectedOwnsLiveCapture && (
          <div className="wp-streaming-autoscroll-tail" aria-hidden="true" />
        )}
      </div>
    </div>
  );
}
