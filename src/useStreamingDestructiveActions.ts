import {
  useCallback,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import {
  clearStreamingSession,
  deleteStreamingSession,
  type StreamingMfu,
  type StreamingWindow,
} from "./ipc";
import { type TranslationEntry } from "./streamingText";

type PartialTranscript = { itemId: string | null; text: string } | null;
type PreviewWork = { inFlight: boolean; latestText: string | null };
type TranslationQueueItem = {
  sessionId: number;
  targetLanguage: "ru" | "en";
  token: number;
  windowIndex: number;
  sourceText: string;
};

/** Owns destructive meeting cleanup and deliberately rejects failures so the
 * shared confirmation can re-enable a visible retry affordance. */
export function useStreamingDestructiveActions({
  activeId,
  activeIdRef,
  deleteTarget,
  refreshSessions,
  setError,
  setActiveSessionEngine,
  setActiveId,
  setWindows,
  setMfu,
  setCraftFailedId,
  setPrettifiedText,
  setPrettifyFailedId,
  setPendingPrettify,
  setTranslationEnabled,
  commitTranslations,
  translationQueueRef,
  translationTokenRef,
  persistedTranslationsRef,
  setPersistedReady,
  setDeleteTarget,
  partialTranscriptRef,
  previewGenerationRef,
  setPartialTranscript,
  previewWorkRef,
  setPartialTranslation,
  setClearPending,
}: {
  activeId: number | null;
  activeIdRef: MutableRefObject<number | null>;
  deleteTarget: { id: number; title: string } | null;
  refreshSessions: () => Promise<void>;
  setError: Dispatch<SetStateAction<string | null>>;
  setActiveSessionEngine: Dispatch<SetStateAction<"local" | "cloud" | null>>;
  setActiveId: Dispatch<SetStateAction<number | null>>;
  setWindows: Dispatch<SetStateAction<StreamingWindow[]>>;
  setMfu: Dispatch<SetStateAction<StreamingMfu | null>>;
  setCraftFailedId: Dispatch<SetStateAction<number | null>>;
  setPrettifiedText: Dispatch<SetStateAction<string | null>>;
  setPrettifyFailedId: Dispatch<SetStateAction<number | null>>;
  setPendingPrettify: Dispatch<
    SetStateAction<{ original: string; cleaned: string } | null>
  >;
  setTranslationEnabled: Dispatch<SetStateAction<boolean>>;
  commitTranslations: (next: Map<number, TranslationEntry>) => void;
  translationQueueRef: MutableRefObject<TranslationQueueItem[]>;
  translationTokenRef: MutableRefObject<number>;
  persistedTranslationsRef: MutableRefObject<
    Map<number, { text: string; sourceText: string }>
  >;
  setPersistedReady: Dispatch<SetStateAction<boolean>>;
  setDeleteTarget: Dispatch<
    SetStateAction<{ id: number; title: string } | null>
  >;
  partialTranscriptRef: MutableRefObject<PartialTranscript>;
  previewGenerationRef: MutableRefObject<number>;
  setPartialTranscript: Dispatch<SetStateAction<PartialTranscript>>;
  previewWorkRef: MutableRefObject<PreviewWork>;
  setPartialTranslation: Dispatch<
    SetStateAction<{ sourceText: string; translatedText?: string } | null>
  >;
  setClearPending: Dispatch<SetStateAction<boolean>>;
}) {
  const confirmDelete = useCallback(async () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setError(null);
    try {
      await deleteStreamingSession(target.id);
      if (activeId === target.id) setActiveSessionEngine(null);
      setActiveId((current) => {
        if (current !== target.id) return current;
        setWindows([]);
        setMfu(null);
        setCraftFailedId((failedId) =>
          failedId === target.id ? null : failedId,
        );
        setPrettifiedText(null);
        setPrettifyFailedId((failedId) =>
          failedId === target.id ? null : failedId,
        );
        setPendingPrettify(null);
        setTranslationEnabled(false);
        commitTranslations(new Map());
        translationQueueRef.current = [];
        translationTokenRef.current += 1;
        persistedTranslationsRef.current = new Map();
        setPersistedReady(false);
        return null;
      });
      await refreshSessions();
      setDeleteTarget(null);
    } catch (error) {
      setError(String(error));
      throw error;
    }
  }, [
    activeId,
    commitTranslations,
    deleteTarget,
    persistedTranslationsRef,
    refreshSessions,
    setActiveId,
    setActiveSessionEngine,
    setCraftFailedId,
    setDeleteTarget,
    setError,
    setMfu,
    setPendingPrettify,
    setPersistedReady,
    setPrettifiedText,
    setPrettifyFailedId,
    setTranslationEnabled,
    setWindows,
    translationQueueRef,
    translationTokenRef,
  ]);

  const confirmClear = useCallback(async () => {
    const id = activeIdRef.current;
    if (id === null) return;
    setError(null);
    try {
      const session = await clearStreamingSession(id);
      if (activeIdRef.current !== id) return;
      setWindows(session.windows);
      setMfu(session.mfu ?? null);
      setPrettifiedText(session.prettified_text ?? null);
      setPendingPrettify(null);
      setCraftFailedId((failedId) => (failedId === id ? null : failedId));
      setPrettifyFailedId((failedId) => (failedId === id ? null : failedId));
      partialTranscriptRef.current = null;
      previewGenerationRef.current += 1;
      setPartialTranscript(null);
      commitTranslations(new Map());
      translationQueueRef.current = [];
      translationTokenRef.current += 1;
      previewWorkRef.current.latestText = null;
      setPartialTranslation(null);
      persistedTranslationsRef.current = new Map();
      setPersistedReady(true);
      setClearPending(false);
      await refreshSessions();
    } catch (error) {
      if (activeIdRef.current === id) setError(String(error));
      throw error;
    }
  }, [
    activeIdRef,
    commitTranslations,
    partialTranscriptRef,
    persistedTranslationsRef,
    previewGenerationRef,
    previewWorkRef,
    refreshSessions,
    setClearPending,
    setCraftFailedId,
    setError,
    setMfu,
    setPartialTranscript,
    setPartialTranslation,
    setPendingPrettify,
    setPersistedReady,
    setPrettifiedText,
    setPrettifyFailedId,
    setWindows,
    translationQueueRef,
    translationTokenRef,
  ]);

  return { confirmDelete, confirmClear };
}
