import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type MutableRefObject,
} from "react";
import {
  getSettings,
  listStreamingTranslations,
  listTaskModels,
  previewStreamingTranslation,
  setStreamingTranslationEnabled,
  setStreamingTranslationTargetLanguage,
  translateStreamingWindow,
  type StreamingTranslationTargetLanguage,
  type StreamingWindow,
} from "./ipc";
import { windowText, type TranslationEntry } from "./streamingText";

type PartialTranscript = { itemId: string | null; text: string } | null;

function precedingWindowsContext(
  windows: StreamingWindow[],
  index: number,
  entries: Map<number, TranslationEntry>,
) {
  const parts: string[] = [];
  for (let i = Math.max(0, index - 2); i < index; i += 1) {
    const window = windows[i];
    const entry = window && entries.get(window.window_index);
    if (
      entry &&
      (entry.status === "done" || entry.status === "mirrored") &&
      entry.translatedText !== undefined
    )
      parts.push(entry.translatedText);
  }
  return parts.length > 0 ? parts.join(" ") : undefined;
}

export function useStreamingTranslationController({
  activeId,
  activeIdRef,
  windows,
  partialTranscript,
  partialTranscriptRef,
  selectedOwnsLiveCapture,
  transcriptionEngine,
  paragraphs,
}: {
  activeId: number | null;
  activeIdRef: MutableRefObject<number | null>;
  windows: StreamingWindow[];
  partialTranscript: PartialTranscript;
  partialTranscriptRef: MutableRefObject<PartialTranscript>;
  selectedOwnsLiveCapture: boolean;
  transcriptionEngine: "local" | "cloud";
  paragraphs: StreamingWindow[][];
}) {
  // WP-93/WP-103: Live Translation — switch state, session-owned target
  // language (default Russian), and per-*window* translation status keyed by
  // window_index (WP-103 moved this off paragraph_key). The queue itself
  // lives in refs (not state) since it's an implementation detail that never
  // renders directly.
  const [translationEnabled, setTranslationEnabled] = useState(false);
  const [translationSelectionColumn, setTranslationSelectionColumn] = useState<
    "source" | "target" | null
  >(null);
  const [targetLanguage, setTargetLanguage] =
    useState<StreamingTranslationTargetLanguage>("ru");
  const targetLanguageRef = useRef<StreamingTranslationTargetLanguage>("ru");
  function commitTargetLanguage(next: StreamingTranslationTargetLanguage) {
    targetLanguageRef.current = next;
    setTargetLanguage(next);
  }
  const [translations, setTranslations] = useState<
    Map<number, TranslationEntry>
  >(new Map());
  // Mirrors `translations` synchronously (unlike React state, which only
  // commits on a later render) so code that runs across an async boundary —
  // the translate queue's dequeue-time context lookup, its .then/.catch
  // result handlers — always reads the freshest map instead of racing a
  // pending re-render. `commitTranslations` is the only way either is
  // written, so they can never drift apart.
  const translationsRef = useRef<Map<number, TranslationEntry>>(new Map());
  function commitTranslations(next: Map<number, TranslationEntry>) {
    translationsRef.current = next;
    setTranslations(next);
  }
  // Mirrors `windows` for the same reason — the queue's dequeue-time context
  // lookup needs each window's position among its neighbors, read fresh at
  // that moment rather than from whatever render defined the closure.
  const windowsRef = useRef<StreamingWindow[]>([]);
  useEffect(() => {
    windowsRef.current = windows;
  }, [windows]);
  const [llmModelReady, setLlmModelReady] = useState(false);
  const translationQueueRef = useRef<
    {
      sessionId: number;
      targetLanguage: StreamingTranslationTargetLanguage;
      token: number;
      windowIndex: number;
      sourceText: string;
    }[]
  >([]);
  const translationBusyRef = useRef(false);
  const [partialTranslation, setPartialTranslation] = useState<{
    sourceText: string;
    translatedText?: string;
  } | null>(null);
  const previewWorkRef = useRef<{
    inFlight: boolean;
    latestText: string | null;
  }>({ inFlight: false, latestText: null });
  const previewGenerationRef = useRef(0);
  // Bumped whenever translation is toggled (either direction) so an
  // in-flight promise from a superseded run discards its result instead of
  // writing into a queue/map that's since been cleared.
  const translationTokenRef = useRef(0);
  const persistedTranslationsRef = useRef<
    Map<number, { text: string; sourceText: string }>
  >(new Map());
  // False while a fresh (session, targetLanguage) persisted-translations
  // fetch is outstanding — gates the reconcile effect below so it never
  // enqueues a live model call before finding out whether a persisted
  // result already covers a window.
  const [persistedReady, setPersistedReady] = useState(false);
  function runLatestPreviewTranslation() {
    const work = previewWorkRef.current;
    if (work.inFlight) return;
    const sourceText = work.latestText;
    work.latestText = null;
    if (!sourceText || partialTranscriptRef.current?.text.trim() !== sourceText)
      return;
    const token = translationTokenRef.current;
    const previewGeneration = previewGenerationRef.current;
    work.inFlight = true;
    setPartialTranslation((current) => ({
      sourceText,
      translatedText: current?.translatedText,
    }));
    const context = precedingWindowsContext(
      windowsRef.current,
      windowsRef.current.length,
      translationsRef.current,
    );
    void previewStreamingTranslation(
      targetLanguageRef.current,
      sourceText,
      context,
    )
      .then((translatedText) => {
        if (
          translationTokenRef.current === token &&
          previewGenerationRef.current === previewGeneration &&
          partialTranscriptRef.current
        ) {
          setPartialTranslation((current) =>
            current === null ? null : { ...current, translatedText },
          );
        }
      })
      .catch(() => {
        // A provisional failure is intentionally silent. The committed
        // window still enters the durable translation queue and owns errors.
      })
      .finally(() => {
        work.inFlight = false;
        if (
          work.latestText &&
          partialTranscriptRef.current?.text.trim() === work.latestText
        ) {
          runLatestPreviewTranslation();
        }
      });
  }

  useEffect(() => {
    partialTranscriptRef.current = partialTranscript;
    const sourceText = partialTranscript?.text.trim() ?? "";
    if (
      !translationEnabled ||
      !selectedOwnsLiveCapture ||
      transcriptionEngine !== "cloud" ||
      !llmModelReady ||
      sourceText.length === 0
    ) {
      previewWorkRef.current.latestText = null;
      setPartialTranslation(null);
      return;
    }
    if (partialTranslation?.sourceText === sourceText) return;
    previewWorkRef.current.latestText = sourceText;
    runLatestPreviewTranslation();
  }, [
    llmModelReady,
    partialTranscript,
    partialTranslation?.sourceText,
    selectedOwnsLiveCapture,
    targetLanguage,
    transcriptionEngine,
    translationEnabled,
  ]);

  // WP-93: Live Translation's model-readiness gate, mirroring how App.tsx's
  // Meeting screen resolves `llmModelReady` for Craft MFU (listTaskModels +
  // getSettings().active_model_llm) — a failure of either call leaves the
  // switch disabled rather than surfacing a blocking error.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [models, settings] = await Promise.all([
          listTaskModels(),
          getSettings(),
        ]);
        if (cancelled) return;
        const llmId = settings.active_model_llm;
        const model = llmId ? models.find((m) => m.id === llmId) : undefined;
        setLlmModelReady(model?.downloaded ?? false);
      } catch {
        if (!cancelled) setLlmModelReady(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // --- WP-93/WP-103: Live Translation --------------------------------------

  // Runs the queue's next item, if any, honoring the single-flight
  // constraint (`translationBusyRef`). Context (WP-103) is read fresh here
  // at dequeue time from `windowsRef`/`translationsRef`, not snapshotted at
  // enqueue time — required for the 2-window bootstrap pair, where window 1
  // is enqueued before window 0 has actually translated.
  function runTranslationQueue() {
    if (translationBusyRef.current) return;
    let item = translationQueueRef.current.shift();
    while (
      item &&
      (item.token !== translationTokenRef.current ||
        item.sessionId !== activeIdRef.current ||
        item.targetLanguage !== targetLanguageRef.current)
    ) {
      item = translationQueueRef.current.shift();
    }
    if (!item) return;
    const sessionId = item.sessionId;
    const lang = item.targetLanguage;
    const token = item.token;
    translationBusyRef.current = true;
    const position = windowsRef.current.findIndex(
      (w) => w.window_index === item.windowIndex,
    );
    const context =
      position >= 0
        ? precedingWindowsContext(
            windowsRef.current,
            position,
            translationsRef.current,
          )
        : undefined;
    {
      const next = new Map(translationsRef.current);
      next.set(item.windowIndex, {
        status: "translating",
        sourceText: item.sourceText,
      });
      commitTranslations(next);
    }
    void translateStreamingWindow(
      sessionId,
      item.windowIndex,
      lang,
      item.sourceText,
      context,
    )
      .then((text) => {
        if (translationTokenRef.current !== token) return;
        const current = translationsRef.current.get(item.windowIndex);
        if (!current || current.sourceText !== item.sourceText) return;
        const next = new Map(translationsRef.current);
        next.set(item.windowIndex, {
          status: "done",
          sourceText: item.sourceText,
          translatedText: text,
        });
        commitTranslations(next);
      })
      .catch(() => {
        if (translationTokenRef.current !== token) return;
        const current = translationsRef.current.get(item.windowIndex);
        if (!current || current.sourceText !== item.sourceText) return;
        const next = new Map(translationsRef.current);
        next.set(item.windowIndex, {
          status: "failed",
          sourceText: item.sourceText,
        });
        commitTranslations(next);
      })
      .finally(() => {
        translationBusyRef.current = false;
        runTranslationQueue();
      });
  }

  // Upserts by window_index so a window whose text changed again before its
  // earlier queued attempt started replaces the stale payload rather than
  // running twice. Windows are always enqueued in increasing window_index
  // order by the reconcile effect and retry below, so the queue — and thus
  // every translate call — processes strictly in that order.
  function enqueueTranslation(windowIndex: number, sourceText: string) {
    const sessionId = activeIdRef.current;
    if (sessionId === null) return;
    const target = targetLanguageRef.current;
    const token = translationTokenRef.current;
    const queue = translationQueueRef.current;
    const index = queue.findIndex(
      (entry) =>
        entry.sessionId === sessionId &&
        entry.targetLanguage === target &&
        entry.windowIndex === windowIndex,
    );
    const item = {
      sessionId,
      targetLanguage: target,
      token,
      windowIndex,
      sourceText,
    };
    if (index >= 0) {
      queue[index] = item;
    } else {
      queue.push(item);
    }
    runTranslationQueue();
  }

  // Clearing translations/queue on both directions (not just OFF) means
  // turning back ON always re-derives fresh from current windows + a fresh
  // persisted-translations fetch, so a stale target-language translation
  // can never be reused. Also resets `persistedReady` synchronously,
  // mirroring the persisted-fetch effect below — without this, a second
  // activation in the same commit would see the previous run's leftover
  // `persistedReady === true` against an already-cleared
  // `persistedTranslationsRef`, queuing every window for an unneeded call.
  const handleToggleTranslation = useCallback((next: boolean) => {
    setTranslationEnabled(next);
    commitTranslations(new Map());
    translationQueueRef.current = [];
    translationTokenRef.current += 1;
    previewWorkRef.current.latestText = null;
    setPartialTranslation(null);
    persistedTranslationsRef.current = new Map();
    if (next) setPersistedReady(false);
    // WP-101: best-effort persistence, mirroring handleToggleMfuPanel — the
    // switch already reflects `next`; a write failure is swallowed rather
    // than surfaced as a blocking error or used to revert the switch.
    const sessionId = activeIdRef.current;
    if (sessionId !== null) {
      void (async () => {
        try {
          await setStreamingTranslationEnabled(sessionId, next);
        } catch {
          // Best-effort persistence: the switch already reflects `next`.
        }
      })();
    }
  }, []);

  const handleTargetLanguageChange = useCallback(
    (next: StreamingTranslationTargetLanguage) => {
      commitTargetLanguage(next);
      const sessionId = activeIdRef.current;
      if (sessionId === null) return;
      void (async () => {
        try {
          await setStreamingTranslationTargetLanguage(sessionId, next);
        } catch {
          // View state remains usable; persistence retries on the next user
          // selection instead of turning this into a blocking capture error.
        }
      })();
    },
    [],
  );

  // WP-103: the retry affordance stays at the paragraph level (one button,
  // not one per window) — re-enqueues every currently-FAILED window within
  // that paragraph, leaving its done/mirrored/pending/translating siblings
  // untouched. Context for each retried window is recomputed at retry time
  // (like any other enqueue) by `runTranslationQueue`'s dequeue-time lookup,
  // from the windows' current state — not from whatever it was originally.
  const handleRetryTranslation = useCallback(
    (paragraphKey: number) => {
      const paragraph = paragraphs.find(
        (p) => p[0].window_index === paragraphKey,
      );
      if (!paragraph) return;
      const failedWindows = paragraph.filter((w) => {
        const entry = translations.get(w.window_index);
        return (
          entry !== undefined &&
          entry.sourceText === windowText(w) &&
          entry.status === "failed"
        );
      });
      if (failedWindows.length === 0) return;
      const next = new Map(translationsRef.current);
      for (const w of failedWindows) {
        next.set(w.window_index, {
          status: "pending",
          sourceText: windowText(w),
        });
      }
      commitTranslations(next);
      for (const w of failedWindows) {
        enqueueTranslation(w.window_index, windowText(w));
      }
    },
    [paragraphs, translations],
  );

  // Loads this session+target-language's persisted translations once per
  // "Live Translation On" so the reconcile effect below can reuse them
  // instead of re-running the model (WP-92's single-flight command makes
  // replaying a whole session's windows on every toggle expensive).
  useEffect(() => {
    if (!translationEnabled || activeId === null) return;
    let cancelled = false;
    setPersistedReady(false);
    const sessionId = activeId;
    const lang = targetLanguage;
    void (async () => {
      const map = new Map<number, { text: string; sourceText: string }>();
      try {
        const rows = await listStreamingTranslations(sessionId, lang);
        for (const row of rows) {
          map.set(row.window_index, {
            text: row.translated_text,
            sourceText: row.source_text,
          });
        }
      } catch {
        // Best-effort: proceed with nothing persisted — windows are
        // translated live instead.
      }
      if (cancelled) return;
      persistedTranslationsRef.current = map;
      setPersistedReady(true);
    })();
    return () => {
      cancelled = true;
    };
  }, [translationEnabled, activeId, targetLanguage]);

  // WP-103: reconciles every window directly against its own translation
  // entry (paragraph grouping stays display-only — see
  // groupWindowsIntoParagraphs' on-screen use below). The first committed
  // window starts immediately with no context; later windows consume the
  // rolling translated context. A failed entry whose source text still
  // matches is left alone — retry is manual only.
  useEffect(() => {
    if (!translationEnabled || activeId === null || !persistedReady) return;
    const next = new Map(translationsRef.current);
    let changed = false;
    const toEnqueue: { windowIndex: number; sourceText: string }[] = [];
    for (const w of windows) {
      const sourceText = windowText(w);
      const existing = next.get(w.window_index);
      if (!w.outcome_ok) {
        if (existing) {
          next.delete(w.window_index);
          changed = true;
        }
        continue;
      }
      const isTargetAlready = w.language.toLowerCase() === targetLanguage;
      if (isTargetAlready) {
        if (
          !existing ||
          existing.sourceText !== sourceText ||
          existing.status !== "mirrored"
        ) {
          next.set(w.window_index, {
            status: "mirrored",
            sourceText,
            translatedText: sourceText,
          });
          changed = true;
        }
        continue;
      }
      const persisted = persistedTranslationsRef.current.get(w.window_index);
      if (persisted && persisted.sourceText === sourceText) {
        if (
          !existing ||
          existing.sourceText !== sourceText ||
          existing.status !== "done" ||
          existing.translatedText !== persisted.text
        ) {
          next.set(w.window_index, {
            status: "done",
            sourceText,
            translatedText: persisted.text,
          });
          changed = true;
        }
        continue;
      }
      if (!existing || existing.sourceText !== sourceText) {
        next.set(w.window_index, { status: "pending", sourceText });
        changed = true;
        toEnqueue.push({ windowIndex: w.window_index, sourceText });
      }
    }
    if (changed) commitTranslations(next);
    for (const item of toEnqueue) {
      enqueueTranslation(item.windowIndex, item.sourceText);
    }
    // `translations` intentionally omitted: read directly from
    // `translationsRef` to decide reuse without re-running this effect on
    // every status transition the queue itself writes (translating/done/
    // failed) — those don't change which windows are stale.
  }, [windows, translationEnabled, activeId, targetLanguage, persistedReady]);

  return {
    translationEnabled,
    setTranslationEnabled,
    translationSelectionColumn,
    setTranslationSelectionColumn,
    targetLanguage,
    commitTargetLanguage,
    translations,
    commitTranslations,
    windowsRef,
    llmModelReady,
    partialTranslation,
    setPartialTranslation,
    translationQueueRef,
    translationTokenRef,
    persistedTranslationsRef,
    setPersistedReady,
    previewGenerationRef,
    previewWorkRef,
    handleToggleTranslation,
    handleTargetLanguageChange,
    handleRetryTranslation,
  };
}
