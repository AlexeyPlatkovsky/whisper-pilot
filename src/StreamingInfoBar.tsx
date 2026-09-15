import { Icon } from "./Icon";
import { sourcesLabel } from "./streamingText";

export function StreamingInfoBar({
  sources,
  durationLabel,
}: {
  sources: { mic: boolean; system_audio: boolean } | null;
  durationLabel: string;
}) {
  return (
    <div className="wp-info-bar">
      <div className="wp-info-left">
        <span className="wp-info-label">Audio Source:</span>
        <span className="wp-file-chip">
          <Icon name="mic" size={14} />
          {sources ? sourcesLabel(sources) : "No audio source"}
        </span>
      </div>
      <div className="wp-info-right">
        <span className="wp-info-meta">
          <Icon name="globe" size={14} />
          Auto-detect (EN/RU/TR)
        </span>
        <span className="wp-info-meta">{durationLabel}</span>
      </div>
    </div>
  );
}
