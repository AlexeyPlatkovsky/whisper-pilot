// A three-segment control switches between the app's Transcription, Meeting,
// and Recorder workspaces. Internal mode values retain their legacy names so
// persisted settings and IPC contracts do not need a migration.

export function ModeToggle({
  mode,
  onSelectMeeting,
  onSelectStreaming,
  onSelectRecorder,
}: {
  mode: "meeting" | "streaming" | "recorder";
  onSelectMeeting: () => void;
  onSelectStreaming: () => void;
  onSelectRecorder?: () => void;
}) {
  return (
    <div className="wp-mode-toggle" role="group" aria-label="Workspace mode">
      <button
        type="button"
        className={`wp-mode-toggle-segment${mode === "meeting" ? " is-active" : ""}`}
        aria-pressed={mode === "meeting"}
        onClick={onSelectMeeting}
      >
        Transcription
      </button>
      <button
        type="button"
        className={`wp-mode-toggle-segment${mode === "streaming" ? " is-active" : ""}`}
        aria-pressed={mode === "streaming"}
        onClick={onSelectStreaming}
      >
        Meeting
      </button>
      <button
        type="button"
        className={`wp-mode-toggle-segment${mode === "recorder" ? " is-active" : ""}`}
        aria-pressed={mode === "recorder"}
        onClick={onSelectRecorder}
      >
        Recorder
      </button>
    </div>
  );
}
