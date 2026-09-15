import {
  useCallback,
  useEffect,
  useLayoutEffect,
  type MutableRefObject,
  type UIEvent,
} from "react";
import type { StreamingWindow } from "./ipc";
import type { TranslationEntry } from "./streamingText";

const RESUME_THRESHOLD_PX = 48;

export function useStreamingPresentationEffects({
  transcriptScrollRef,
  autoScrollEnabledRef,
  selectedOwnsLiveCapture,
  partialTranscript,
  translations,
  windows,
  isCraftingActive,
  isPrettifyingActive,
  captureElapsedBaselineRef,
  startTimeRef,
  setElapsed,
}: {
  transcriptScrollRef: MutableRefObject<HTMLDivElement | null>;
  autoScrollEnabledRef: MutableRefObject<boolean>;
  selectedOwnsLiveCapture: boolean;
  partialTranscript: unknown;
  translations: Map<number, TranslationEntry>;
  windows: StreamingWindow[];
  isCraftingActive: boolean;
  isPrettifyingActive: boolean;
  captureElapsedBaselineRef: MutableRefObject<number>;
  startTimeRef: MutableRefObject<number | null>;
  setElapsed: (seconds: number) => void;
}) {
  const handleTranscriptScroll = useCallback(
    (event: UIEvent<HTMLDivElement>) => {
      const container = event.currentTarget;
      const distance = Math.max(
        0,
        container.scrollHeight - container.clientHeight - container.scrollTop,
      );
      autoScrollEnabledRef.current = distance <= RESUME_THRESHOLD_PX;
    },
    [autoScrollEnabledRef],
  );

  useLayoutEffect(() => {
    const container = transcriptScrollRef.current;
    if (selectedOwnsLiveCapture && autoScrollEnabledRef.current && container)
      container.scrollTop = container.scrollHeight;
  }, [
    autoScrollEnabledRef,
    partialTranscript,
    selectedOwnsLiveCapture,
    transcriptScrollRef,
    translations,
    windows,
  ]);

  useEffect(() => {
    if (!selectedOwnsLiveCapture && !isCraftingActive && !isPrettifyingActive)
      return;
    const baseline = selectedOwnsLiveCapture
      ? captureElapsedBaselineRef.current
      : 0;
    startTimeRef.current = Date.now() - baseline * 1000;
    setElapsed(baseline);
    const id = setInterval(
      () => setElapsed(Math.floor((Date.now() - startTimeRef.current!) / 1000)),
      1000,
    );
    return () => clearInterval(id);
  }, [
    captureElapsedBaselineRef,
    isCraftingActive,
    isPrettifyingActive,
    selectedOwnsLiveCapture,
    setElapsed,
    startTimeRef,
  ]);

  return handleTranscriptScroll;
}
