import { ActionIcon } from "./ActionIcon";
import { CopyButton } from "./CopyButton";

export function RecorderActionGroup({
  transcript,
  resetKey,
  startDisabled,
  stopDisabled,
  prettifyDisabled,
  exportDisabled,
  clearDisabled,
  onStart,
  onStop,
  onPrettify,
  onExport,
  onClear,
  onError,
}: {
  transcript: string;
  resetKey: number | null;
  startDisabled: boolean;
  stopDisabled: boolean;
  prettifyDisabled: boolean;
  exportDisabled: boolean;
  clearDisabled: boolean;
  onStart: () => void;
  onStop: () => void;
  onPrettify: () => void;
  onExport: () => void;
  onClear: () => void;
  onError: (message: string | null) => void;
}) {
  return (
    <div className="wp-action-group">
      <ActionIcon
        icon="play"
        label="Start"
        onClick={onStart}
        disabled={startDisabled}
      />
      <span className="wp-sep" />
      <ActionIcon
        icon="square"
        label="Stop"
        onClick={onStop}
        disabled={stopDisabled}
      />
      <span className="wp-sep" />
      <ActionIcon
        icon="sparkles"
        label="Prettify transcript"
        accent
        onClick={onPrettify}
        disabled={prettifyDisabled}
      />
      <span className="wp-sep" />
      <CopyButton
        text={transcript}
        resetKey={resetKey}
        onError={onError}
        onCopied={() => onError(null)}
        disabled={!transcript.trim()}
      />
      <span className="wp-sep" />
      <ActionIcon
        icon="download"
        label="Export transcript"
        onClick={onExport}
        disabled={exportDisabled}
      />
      <span className="wp-sep" />
      <ActionIcon
        icon="eraser"
        label="Clear recording"
        onClick={onClear}
        disabled={clearDisabled}
      />
    </div>
  );
}
