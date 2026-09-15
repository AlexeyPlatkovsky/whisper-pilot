import type { Dispatch, SetStateAction } from "react";
import { RecorderView } from "./RecorderView";
import { SettingsLayer } from "./SettingsLayer";
import { StreamingView } from "./StreamingView";

/** Owns mutually exclusive top-level workspace modes; App retains their state. */
export function AppModeView({
  mode,
  settingsOpen,
  meetingTranscriptionActive,
  recorderStartPending,
  onRecorderStartPendingChange,
  isStreamingActive,
  onSelectMeeting,
  onCloseStreaming,
  onSelectStreaming,
  onSelectRecorder,
  onOpenSettings,
  onCloseSettings,
}: {
  mode: "recorder" | "streaming" | "settings";
  settingsOpen: boolean;
  meetingTranscriptionActive: boolean;
  recorderStartPending: boolean;
  onRecorderStartPendingChange: Dispatch<SetStateAction<boolean>>;
  isStreamingActive: boolean;
  onSelectMeeting: () => void;
  onCloseStreaming: () => void;
  onSelectStreaming: () => void;
  onSelectRecorder: () => void;
  onOpenSettings: () => void;
  onCloseSettings: () => void;
}) {
  if (mode === "recorder") {
    return (
      <>
        <RecorderView
          onSelectMeeting={onSelectMeeting}
          onSelectStreaming={onSelectStreaming}
          onOpenSettings={onOpenSettings}
          meetingTranscriptionActive={meetingTranscriptionActive}
          recorderStartPending={recorderStartPending}
          onRecorderStartPendingChange={onRecorderStartPendingChange}
        />
        <SettingsLayer
          open={settingsOpen}
          overlay
          cloudProviderLocked={isStreamingActive}
          onClose={onCloseSettings}
        />
      </>
    );
  }
  if (mode === "streaming") {
    return (
      <>
        <StreamingView
          onClose={onCloseStreaming}
          onOpenSettings={onOpenSettings}
          settingsOpen={settingsOpen}
          meetingTranscriptionActive={meetingTranscriptionActive}
          onSelectRecorder={onSelectRecorder}
        />
        <SettingsLayer
          open={settingsOpen}
          overlay
          cloudProviderLocked={isStreamingActive}
          onClose={onCloseSettings}
        />
      </>
    );
  }
  return (
    <div className="app">
      <SettingsLayer
        open={settingsOpen}
        cloudProviderLocked={isStreamingActive}
        onClose={onCloseSettings}
      />
    </div>
  );
}
