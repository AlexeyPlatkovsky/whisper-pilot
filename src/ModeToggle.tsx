// A two-segment control switching between the app's two top-level windows.
// Each window is still its own top-level view (mirroring `pencil/main_view.pen`'s
// separate "WhisperPilot Window" / "Streaming Window" frames) — this replaces
// the header's old, separate "Streaming" icon button as the way to move
// between them.

export function ModeToggle({
  mode,
  onSelectMeeting,
  onSelectStreaming,
}: {
  mode: "meeting" | "streaming";
  onSelectMeeting: () => void;
  onSelectStreaming: () => void;
}) {
  return (
    <div className="wp-mode-toggle" role="group" aria-label="Workspace mode">
      <button
        type="button"
        className={`wp-mode-toggle-segment${mode === "meeting" ? " is-active" : ""}`}
        aria-pressed={mode === "meeting"}
        onClick={onSelectMeeting}
      >
        Meeting
      </button>
      <button
        type="button"
        className={`wp-mode-toggle-segment${mode === "streaming" ? " is-active" : ""}`}
        aria-pressed={mode === "streaming"}
        onClick={onSelectStreaming}
      >
        Streaming
      </button>
    </div>
  );
}
