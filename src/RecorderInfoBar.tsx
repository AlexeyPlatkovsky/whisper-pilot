import { convertFileSrc } from "@tauri-apps/api/core";
import type { RecorderSession } from "./ipc";
import { formatDuration } from "./format";
import { Icon } from "./Icon";

export function RecorderInfoBar({
  active,
  onExportWav,
}: {
  active: RecorderSession | null;
  onExportWav: () => void;
}) {
  const audioPath =
    active?.status === "completed" && !active.is_draft
      ? active.audio_path
      : undefined;
  return (
    <div className="wp-info-bar recorder-info-bar">
      <div className="wp-info-left recorder-info-left">
        <span className="wp-info-label">Audio Source:</span>
        <span className="wp-file-chip">
          <Icon name="mic" size={14} />
          Default microphone
        </span>
        {audioPath && (
          <div className="recorder-audio-tools">
            <audio
              className="recorder-audio wp-audio-player--header"
              aria-label="Recording playback"
              controls
              preload="metadata"
              src={convertFileSrc(audioPath)}
            >
              Saved Recorder audio
            </audio>
            <button
              type="button"
              className="wp-icon-btn recorder-wav-export"
              aria-label="Export WAV"
              title="Export WAV audio file"
              onClick={onExportWav}
            >
              <Icon name="file-wav" size={19} />
            </button>
          </div>
        )}
      </div>
      <div className="wp-info-right">
        <span className="wp-info-meta">
          {active ? formatDuration(active.duration_ms) : "—"}
        </span>
      </div>
    </div>
  );
}
