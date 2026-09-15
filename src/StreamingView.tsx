import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  acceptStreamingPrettify,
  createStreamingSession,
  generateStreamingMfu,
  generateStreamingPrettify,
  openStreamingSession,
  renameStreamingSession,
  revertStreamingPrettify,
  saveTextDialog,
  setStreamingTranslationEnabled,
  setStreamingTranslationTargetLanguage,
  startStreamingSession,
  type LiveCaptureSnapshot,
  type StreamingMfu,
  type StreamingWindow,
} from "./ipc";
import {
  hasStreamingTranslations,
  renderStreamingPaired,
  STREAMING_TARGET_LANGUAGE_NAMES,
} from "./export";
import { useStreamingTranslationController } from "./useStreamingTranslationController";
import { StreamingViewBody } from "./StreamingViewBody";
import { useStreamingPresentationEffects } from "./useStreamingPresentationEffects";
import { useStreamingPreferences } from "./useStreamingPreferences";
import { useStreamingSessionLibrary } from "./useStreamingSessionLibrary";
import { useStreamingEventListeners } from "./useStreamingEventListeners";
import { useStreamingCaptureLifecycle } from "./useStreamingCaptureLifecycle";
import { useStreamingCaptureActions } from "./useStreamingCaptureActions";
import { groupWindowsIntoParagraphs } from "./paragraphs";
import { fileNameFor, plainTranscript, toMarkdown } from "./streamingText";
import { useStreamingViewDerived } from "./useStreamingViewDerived";
import { streamingDurationLabel } from "./streamingViewUtils";
import { useStreamingDestructiveActions } from "./useStreamingDestructiveActions";
export function StreamingView({
  onClose,
  onOpenSettings,
  onSelectRecorder,
  settingsOpen = false,
  meetingTranscriptionActive = false,
}: {
  onClose: () => void;
  onOpenSettings: () => void;
  onSelectRecorder?: () => void;
  settingsOpen?: boolean;
  meetingTranscriptionActive?: boolean;
}) {
  const { sessions, sessionsHydrated, refreshSessions } =
    useStreamingSessionLibrary();
  const [sessionSearch, setSessionSearch] = useState("");
  const [activeId, setActiveId] = useState<number | null>(null);
  const [activeTitle, setActiveTitle] = useState<string>("Meeting");
  const [windows, setWindows] = useState<StreamingWindow[]>([]);
  const transcriptScrollRef = useRef<HTMLDivElement | null>(null);
  const autoScrollEnabledRef = useRef(true);
  const [isRunning, setIsRunning] = useState(false);
  const [captureHydrated, setCaptureHydrated] = useState(false);
  const [liveCapturePhase, setLiveCapturePhase] =
    useState<LiveCaptureSnapshot["phase"]>("idle");
  const [liveCaptureSessionId, setLiveCaptureSessionId] = useState<
    number | null
  >(null);
  const liveCaptureSnapshotRef = useRef<LiveCaptureSnapshot | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [sources, setSources] = useState<{
    mic: boolean;
    system_audio: boolean;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [partialTranscript, setPartialTranscript] = useState<{
    itemId: string | null;
    text: string;
  } | null>(null);
  const partialTranscriptRef = useRef<typeof partialTranscript>(null);
  const [transcriptionEngine, setTranscriptionEngine] = useState<
    "local" | "cloud"
  >("local");
  const [activeSessionEngine, setActiveSessionEngine] = useState<
    "local" | "cloud" | null
  >(null);
  const [isStartPending, setIsStartPending] = useState(false);
  const [isStopPending, setIsStopPending] = useState(false);
  const [busy, setBusy] = useState(false);
  const {
    mfuPanelVisible,
    cloudConfiguration,
    refreshCloudConfiguration,
    handleToggleMfuPanel,
  } = useStreamingPreferences(settingsOpen);
  const [elapsed, setElapsed] = useState(0);
  const startTimeRef = useRef<number | null>(null);
  const captureElapsedBaselineRef = useRef(0);
  const [craftingId, setCraftingId] = useState<number | null>(null);
  const [mfu, setMfu] = useState<StreamingMfu | null>(null);
  const [craftFailedId, setCraftFailedId] = useState<number | null>(null);
  const [prettifyingId, setPrettifyingId] = useState<number | null>(null);
  const [prettifyFailedId, setPrettifyFailedId] = useState<number | null>(null);
  const [prettifiedText, setPrettifiedText] = useState<string | null>(null);
  const [pendingPrettify, setPendingPrettify] = useState<{
    original: string;
    cleaned: string;
  } | null>(null);
  const [renameTarget, setRenameTarget] = useState<{
    id: number;
    title: string;
  } | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [renameError, setRenameError] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{
    id: number;
    title: string;
  } | null>(null);
  const [clearPending, setClearPending] = useState(false);
  const activeIdRef = useRef<number | null>(null);
  const initialSelectionAttemptedRef = useRef(false);
  const openRequestRef = useRef(0);
  useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);
  const paragraphs = useMemo(
    () => groupWindowsIntoParagraphs(windows),
    [windows],
  );
  const {
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
  } = useStreamingTranslationController({
    activeId,
    activeIdRef,
    windows,
    partialTranscript,
    partialTranscriptRef,
    selectedOwnsLiveCapture:
      isRunning && activeId !== null && activeId === liveCaptureSessionId,
    transcriptionEngine,
    paragraphs,
  });
  const {
    selectedOwnsLiveCapture,
    isCraftingActive,
    isPrettifyingActive,
    widget,
    liveCaptureWidget,
    filteredSessions,
  } = useStreamingViewDerived({
    sessions,
    sessionSearch,
    activeId,
    liveCaptureSessionId,
    isRunning,
    liveCapturePhase,
    busy,
    craftingId,
    prettifyingId,
    craftFailedId,
    prettifyFailedId,
  });
  const hasLlmWork = craftingId !== null || prettifyingId !== null;
  const hasVisiblePartial =
    partialTranscript !== null && partialTranscript.text.trim().length > 0;
  const handleTranscriptScroll = useStreamingPresentationEffects({
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
  });
  useStreamingCaptureLifecycle({
    activeIdRef,
    liveSnapshotRef: liveCaptureSnapshotRef,
    openRequestRef,
    captureElapsedBaselineRef,
    startTimeRef,
    setCaptureHydrated,
    setLiveCapturePhase,
    setIsRunning,
    setLiveCaptureSessionId,
    setActiveId,
    setActiveTitle,
    setActiveSessionEngine,
    setTranscriptionEngine,
    setWindows,
    setTranslationEnabled,
    commitTargetLanguage,
    setElapsed,
    setError,
  });
  useStreamingEventListeners({
    activeIdRef,
    partialTranscriptRef,
    previewGenerationRef,
    setActiveId,
    setWindows,
    setPartialTranscript,
    setSources,
    setError,
    refreshSessions,
  });

  const startSession = useCallback(
    async (resumeId: number | null, engine: "local" | "cloud") => {
      setError(null);
      if (resumeId === null) autoScrollEnabledRef.current = true;
      captureElapsedBaselineRef.current =
        resumeId === null
          ? 0
          : Math.floor((windowsRef.current.at(-1)?.end_ms ?? 0) / 1000);
      try {
        const isSameSessionResume =
          resumeId !== null && resumeId === activeIdRef.current;
        const summary =
          engine === "cloud"
            ? await startStreamingSession(resumeId ?? undefined, engine)
            : await startStreamingSession(resumeId ?? undefined);
        setActiveId(summary.id);
        setActiveTitle(summary.title);
        setActiveSessionEngine(engine);
        if (resumeId === null) {
          setWindows([]);
          try {
            await Promise.all([
              setStreamingTranslationTargetLanguage(summary.id, targetLanguage),
              setStreamingTranslationEnabled(summary.id, translationEnabled),
            ]);
          } catch {
            // Capture remains authoritative when its metadata write fails.
          }
        }
        setSources(null);
        setPartialTranscript(null);
        setMfu(null);
        setCraftFailedId((current) =>
          current === summary.id ? null : current,
        );
        setPrettifiedText(null);
        setPrettifyFailedId((current) =>
          current === summary.id ? null : current,
        );
        setPendingPrettify(null);
        if (!isSameSessionResume) {
          commitTranslations(new Map());
          translationQueueRef.current = [];
          translationTokenRef.current += 1;
          persistedTranslationsRef.current = new Map();
          setPersistedReady(false); // WP-102: see handleOpen.
        }
        await refreshSessions();
      } catch (e) {
        setError(String(e));
      }
    },
    [refreshSessions, targetLanguage, translationEnabled],
  );

  const { handleStart, handleStop } = useStreamingCaptureActions({
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
  });

  const handleCreateNew = useCallback(async () => {
    setError(null);
    autoScrollEnabledRef.current = true;
    setBusy(true);
    try {
      const summary = await createStreamingSession();
      setActiveId(summary.id);
      setActiveTitle(summary.title);
      setActiveSessionEngine(null);
      setWindows([]);
      setSources(null);
      setMfu(null);
      setPrettifiedText(null);
      setPendingPrettify(null);
      setPartialTranscript(null);
      setTranslationEnabled(false);
      commitTargetLanguage(summary.translation_target_language ?? "ru");
      commitTranslations(new Map());
      translationQueueRef.current = [];
      translationTokenRef.current += 1;
      persistedTranslationsRef.current = new Map();
      setPersistedReady(false); // WP-102: see handleOpen.
      await refreshSessions();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [refreshSessions]);

  const handleOpen = useCallback(async (id: number) => {
    initialSelectionAttemptedRef.current = true;
    const request = ++openRequestRef.current;
    setError(null);
    autoScrollEnabledRef.current = true;
    try {
      const session = await openStreamingSession(id);
      if (request !== openRequestRef.current) return;
      setActiveId(id);
      setActiveTitle(session.title);
      setActiveSessionEngine(session.transcription_engine ?? null);
      if (session.transcription_engine) {
        setTranscriptionEngine(session.transcription_engine);
      }
      setWindows(session.windows);
      setSources(null);
      setMfu(session.mfu ?? null);
      setPrettifiedText(session.prettified_text ?? null);
      setPendingPrettify(null);
      setPartialTranscript(null);
      setTranslationEnabled(session.translation_enabled ?? false);
      commitTargetLanguage(session.translation_target_language ?? "ru");
      commitTranslations(new Map());
      translationQueueRef.current = [];
      translationTokenRef.current += 1;
      persistedTranslationsRef.current = new Map();
      setPersistedReady(false);
    } catch (e) {
      if (request === openRequestRef.current) setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (
      !sessionsHydrated ||
      !captureHydrated ||
      initialSelectionAttemptedRef.current
    ) {
      return;
    }
    initialSelectionAttemptedRef.current = true;
    if (activeIdRef.current !== null) return;
    const first = sessions[0];
    if (first) void handleOpen(first.id);
  }, [captureHydrated, handleOpen, sessions, sessionsHydrated]);

  function openRename(id: number, title: string) {
    setRenameTarget({ id, title });
    setRenameDraft(title);
    setRenameError(null);
  }

  function closeRename() {
    setRenameTarget(null);
    setRenameError(null);
  }

  const submitRename = useCallback(
    async (event: React.FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      if (!renameTarget) return;
      const title = renameDraft.trim();
      if (!title) {
        setRenameError("Meeting title is required");
        return;
      }
      if (Array.from(title).length > 120) {
        setRenameError("Meeting title must be 120 characters or fewer");
        return;
      }
      setError(null);
      try {
        await renameStreamingSession(renameTarget.id, title);
        if (activeId === renameTarget.id) setActiveTitle(title);
        await refreshSessions();
        closeRename();
      } catch (e) {
        setError(String(e));
      }
    },
    [renameTarget, renameDraft, activeId, refreshSessions],
  );

  function openDelete(id: number, title: string) {
    setDeleteTarget({ id, title });
  }

  const { confirmDelete, confirmClear } = useStreamingDestructiveActions({
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
  });

  const plainText = useMemo(() => plainTranscript(windows), [windows]);
  const exportText = useMemo(
    () =>
      prettifiedText ??
      (translationEnabled &&
      (hasStreamingTranslations(translations) ||
        windows.some((window) => !window.outcome_ok))
        ? renderStreamingPaired(paragraphs, translations, targetLanguage)
        : plainText),
    [
      paragraphs,
      plainText,
      prettifiedText,
      targetLanguage,
      translationEnabled,
      translations,
      windows,
    ],
  );

  const handleExport = useCallback(async () => {
    setError(null);
    try {
      await saveTextDialog(
        toMarkdown(activeTitle, exportText),
        fileNameFor(activeTitle),
      );
    } catch (e) {
      setError(String(e));
    }
  }, [exportText, activeTitle]);

  const handleCraft = useCallback(async () => {
    // c8 ignore next -- the action is not rendered until an active session exists.
    if (activeId === null) return;
    const id = activeId;
    handleToggleMfuPanel(true);
    setError(null);
    setCraftFailedId((failedId) => (failedId === id ? null : failedId));
    setCraftingId(id);
    try {
      const session = await generateStreamingMfu(id);
      if (activeIdRef.current === id) {
        setMfu(session.mfu ?? null);
      }
    } catch (e) {
      setCraftFailedId(id);
      if (activeIdRef.current === id) {
        setError(String(e));
      }
    } finally {
      setCraftingId((current) => (current === id ? null : current));
    }
  }, [activeId, handleToggleMfuPanel]);

  const handlePrettify = useCallback(async () => {
    // c8 ignore next -- the action is not rendered until an active session exists.
    if (activeId === null) return;
    const id = activeId;
    const original = plainText;
    setError(null);
    setPrettifyFailedId((failedId) => (failedId === id ? null : failedId));
    setPrettifyingId(id);
    try {
      const cleaned = await generateStreamingPrettify(id);
      if (activeIdRef.current === id) {
        setPendingPrettify({ original, cleaned });
      }
    } catch (e) {
      setPrettifyFailedId(id);
      if (activeIdRef.current === id) {
        setError(String(e));
      }
    } finally {
      setPrettifyingId((current) => (current === id ? null : current));
    }
  }, [activeId, plainText]);

  const handleAcceptPrettify = useCallback(async () => {
    // c8 ignore next -- Accept is only rendered while a review is pending.
    if (activeId === null || !pendingPrettify) return;
    const id = activeId;
    const text = pendingPrettify.cleaned;
    setError(null);
    setPrettifyingId(id);
    try {
      const session = await acceptStreamingPrettify(id, text);
      if (activeIdRef.current === id) {
        setPrettifiedText(session.prettified_text ?? null);
        setPendingPrettify(null);
      }
    } catch (e) {
      if (activeIdRef.current === id) {
        setError(String(e));
      }
    } finally {
      setPrettifyingId((current) => (current === id ? null : current));
    }
  }, [activeId, pendingPrettify]);

  const handleCancelPrettify = useCallback(() => {
    setPendingPrettify(null);
  }, []);

  const handleRevertPrettify = useCallback(async () => {
    // c8 ignore next -- Revert is only rendered while accepted text exists.
    if (activeId === null || prettifiedText === null) return;
    const id = activeId;
    setError(null);
    setPrettifyingId(id);
    try {
      const session = await revertStreamingPrettify(id);
      if (activeIdRef.current === id) {
        setPrettifiedText(session.prettified_text ?? null);
      }
    } catch (e) {
      if (activeIdRef.current === id) setError(String(e));
    } finally {
      setPrettifyingId((current) => (current === id ? null : current));
    }
  }, [activeId, prettifiedText]);

  const headerLocked = !captureHydrated || isRunning || isStartPending;
  const translationDisabledReason = headerLocked
    ? "Meeting controls are unavailable while capture is active."
    : !llmModelReady
      ? "Live Translation needs a downloaded language model."
      : prettifiedText !== null
        ? "Turn off Prettify to use Live Translation."
        : pendingPrettify !== null
          ? "Finish or cancel the Prettify review to use Live Translation."
          : null;
  const prettifyDisabledByTranslation = translationEnabled;

  const hasText = plainText.length > 0;
  const hasCraftableText = windows.some(
    (w) => w.outcome_ok && w.text.length > 0,
  );
  const canPrettify = headerLocked || !hasCraftableText || hasLlmWork;

  const durationLabel = streamingDurationLabel(windows);

  const activeBusy = isRunning || hasLlmWork;
  const selectedCloudProvider = cloudConfiguration?.providers.find(
    (provider) => provider.id === cloudConfiguration.selected_provider,
  );

  return (
    <StreamingViewBody
      view={{
        header: {
          activeId,
          activeTitle,
          activeBusy,
          captureHydrated,
          busy,
          isRunning,
          isStartPending,
          isStopPending,
          meetingTranscriptionActive,
          sidebarOpen,
          elapsed,
          widget,
          windowsCount: windows.length,
          exportText,
          hasText,
          craftDisabled: canPrettify,
          canClear:
            activeId !== null &&
            !activeBusy &&
            (windows.length > 0 || mfu !== null || prettifiedText !== null),
          onToggleSidebar: () => setSidebarOpen((value) => !value),
          onCreate: () => void handleCreateNew(),
          onOpenSettings,
          onRename: () =>
            activeId !== null && openRename(activeId, activeTitle),
          onDelete: () =>
            activeId !== null && openDelete(activeId, activeTitle),
          onStart: () => void handleStart(),
          onStop: () => void handleStop(),
          onCraft: () => void handleCraft(),
          onExport: () => void handleExport(),
          onClear: () => setClearPending(true),
          onError: setError,
        },
        info: { sources, durationLabel },
        sidebar: sidebarOpen
          ? {
              sessions,
              filteredSessions,
              sessionSearch,
              activeId,
              liveCaptureSessionId,
              isRunning,
              liveCaptureWidget,
              activeWidget: widget,
              onSearchChange: setSessionSearch,
              onOpen: (id) => void handleOpen(id),
              onRename: openRename,
              onDelete: openDelete,
              onSelectMeeting: onClose,
              onSelectRecorder,
            }
          : null,
        transcript: {
          view: {
            headerLocked,
            transcriptionEngine,
            setError,
            setTranscriptionEngine,
            translationEnabled,
            handleToggleTranslation,
            translationDisabledReason,
            targetLanguage,
            handleTargetLanguageChange,
            pendingPrettify,
            handleAcceptPrettify,
            handleCancelPrettify,
            prettifiedText,
            handleRevertPrettify,
            prettifyDisabledByTranslation,
            handlePrettify,
            canPrettify,
            mfuPanelVisible,
            handleToggleMfuPanel,
            selectedCloudProvider,
            transcriptScrollRef,
            translationSelectionColumn,
            setTranslationSelectionColumn,
            handleTranscriptScroll,
            error,
            windows,
            selectedOwnsLiveCapture,
            hasVisiblePartial,
            paragraphs,
            translations,
            handleRetryTranslation,
            partialTranscript,
            partialTranslation,
            TARGET_LANGUAGE_NAMES: STREAMING_TARGET_LANGUAGE_NAMES,
          },
        },
        mfu: mfuPanelVisible ? { mfu } : null,
        dialogs: {
          renameTarget,
          renameDraft,
          renameError,
          onRenameDraftChange: (value) => {
            setRenameDraft(value);
            setRenameError(null);
          },
          onRenameCancel: closeRename,
          onSaveRename: submitRename,
          deleteTarget,
          onDeleteCancel: () => setDeleteTarget(null),
          onDelete: confirmDelete,
          clearPending,
          hasActiveSession: activeId !== null,
          onClearCancel: () => setClearPending(false),
          onClear: confirmClear,
        },
      }}
    />
  );
}
