import {
  useEffect,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import {
  onStreamingError,
  onStreamingPartial,
  onStreamingSessionEnded,
  onStreamingSources,
  onStreamingWindow,
  type StreamingWindow,
} from "./ipc";
import { upsertWindow } from "./streamingText";

type PartialTranscript = { itemId: string | null; text: string } | null;

export function useStreamingEventListeners({
  activeIdRef,
  partialTranscriptRef,
  previewGenerationRef,
  setActiveId,
  setWindows,
  setPartialTranscript,
  setSources,
  setError,
  refreshSessions,
}: {
  activeIdRef: MutableRefObject<number | null>;
  partialTranscriptRef: MutableRefObject<PartialTranscript>;
  previewGenerationRef: MutableRefObject<number>;
  setActiveId: Dispatch<SetStateAction<number | null>>;
  setWindows: Dispatch<SetStateAction<StreamingWindow[]>>;
  setPartialTranscript: Dispatch<SetStateAction<PartialTranscript>>;
  setSources: Dispatch<
    SetStateAction<{ mic: boolean; system_audio: boolean } | null>
  >;
  setError: Dispatch<SetStateAction<string | null>>;
  refreshSessions: () => Promise<void>;
}) {
  useEffect(() => {
    let cancelled = false;
    const unlisteners: (() => void)[] = [];
    const dispose = () => {
      for (const unlisten of unlisteners.splice(0)) unlisten();
    };
    const subscribe = async (registration: Promise<() => void>) => {
      const unlisten = await registration;
      if (cancelled) {
        unlisten();
        return false;
      }
      unlisteners.push(unlisten);
      return true;
    };
    void (async () => {
      try {
        if (
          !(await subscribe(
            onStreamingWindow((incoming) => {
              if (cancelled) return;
              const partial = partialTranscriptRef.current;
              if (
                activeIdRef.current === incoming.session_id &&
                partial !== null &&
                (incoming.item_id == null ||
                  incoming.item_id === partial.itemId)
              ) {
                previewGenerationRef.current += 1;
                partialTranscriptRef.current = null;
              }
              setActiveId((current) => {
                if (current === incoming.session_id) {
                  setWindows((windows) => upsertWindow(windows, incoming));
                  setPartialTranscript((currentPartial) => {
                    if (
                      currentPartial === null ||
                      (incoming.item_id != null &&
                        incoming.item_id !== currentPartial.itemId)
                    )
                      return currentPartial;
                    partialTranscriptRef.current = null;
                    return null;
                  });
                }
                return current;
              });
            }),
          ))
        ) {
          return;
        }
        if (
          !(await subscribe(
            onStreamingSources((incoming) => {
              if (cancelled) return;
              setSources({
                mic: incoming.mic,
                system_audio: incoming.system_audio,
              });
            }),
          ))
        ) {
          return;
        }
        if (
          !(await subscribe(
            onStreamingSessionEnded((incoming) => {
              if (cancelled) return;
              if (activeIdRef.current === incoming.session_id) {
                previewGenerationRef.current += 1;
                partialTranscriptRef.current = null;
                setPartialTranscript(null);
              }
              // Session-ended can arrive while the store is briefly busy. Keep
              // the event handler fire-and-forget without leaking a rejection.
              void refreshSessions().catch(() => undefined);
            }),
          ))
        ) {
          return;
        }
        if (
          !(await subscribe(
            onStreamingPartial((incoming) => {
              if (cancelled) return;
              if (activeIdRef.current !== incoming.session_id) return;
              const partial = { itemId: incoming.item_id, text: incoming.text };
              partialTranscriptRef.current = partial;
              setPartialTranscript(partial);
            }),
          ))
        ) {
          return;
        }
        if (
          !(await subscribe(
            onStreamingError((incoming) => {
              if (cancelled) return;
              if (activeIdRef.current === incoming.session_id)
                setError(incoming.message);
            }),
          ))
        ) {
          return;
        }
      } catch {
        dispose();
      }
    })();
    return () => {
      cancelled = true;
      dispose();
    };
  }, [
    activeIdRef,
    partialTranscriptRef,
    previewGenerationRef,
    refreshSessions,
    setActiveId,
    setError,
    setPartialTranscript,
    setSources,
    setWindows,
  ]);
}
