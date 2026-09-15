import { useCallback, type Dispatch, type SetStateAction } from "react";
import { stopStreamingSession, type CloudProviderConfiguration } from "./ipc";

interface StreamingCaptureActionsProps {
  meetingTranscriptionActive: boolean;
  transcriptionEngine: "local" | "cloud";
  refreshCloudConfiguration: () => Promise<CloudProviderConfiguration | null>;
  activeSessionEngine: "local" | "cloud" | null;
  activeId: number | null;
  isRunning: boolean;
  startSession: (
    resumeId: number | null,
    engine: "local" | "cloud",
  ) => Promise<void>;
  setIsStartPending: Dispatch<SetStateAction<boolean>>;
  setIsStopPending: Dispatch<SetStateAction<boolean>>;
  setError: Dispatch<SetStateAction<string | null>>;
}

export function useStreamingCaptureActions({
  meetingTranscriptionActive,
  transcriptionEngine,
  refreshCloudConfiguration,
  activeSessionEngine,
  activeId,
  isRunning,
  startSession,
  setIsStartPending,
  setIsStopPending,
  setError,
}: StreamingCaptureActionsProps) {
  const handleStart = useCallback(async () => {
    if (meetingTranscriptionActive) return;
    if (transcriptionEngine === "cloud") {
      const configuration = await refreshCloudConfiguration();
      const selected = configuration?.providers.find(
        (provider) => provider.id === configuration.selected_provider,
      );
      if (!selected?.configured) {
        setError(
          `Configure a ${selected?.name ?? "Cloud provider"} API key in Settings before starting Cloud transcription.`,
        );
        return;
      }
    }
    const engineChanged =
      activeSessionEngine !== null &&
      activeSessionEngine !== transcriptionEngine;
    const resumeId =
      activeId !== null && !isRunning && !engineChanged ? activeId : null;
    setIsStartPending(true);
    try {
      await startSession(resumeId, transcriptionEngine);
    } finally {
      setIsStartPending(false);
    }
  }, [
    activeId,
    activeSessionEngine,
    isRunning,
    meetingTranscriptionActive,
    refreshCloudConfiguration,
    setError,
    setIsStartPending,
    startSession,
    transcriptionEngine,
  ]);

  const handleStop = useCallback(async () => {
    setError(null);
    setIsStopPending(true);
    try {
      await stopStreamingSession();
    } catch (error) {
      setError(String(error));
    } finally {
      setIsStopPending(false);
    }
  }, [setError, setIsStopPending]);

  return { handleStart, handleStop };
}
