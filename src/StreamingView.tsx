import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  acceptStreamingPrettify,
  createStreamingSession,
  deleteStreamingSession,
  generateStreamingMfu,
  generateStreamingPrettify,
  getCloudProviderConfig,
  getSettings,
  listStreamingSessions,
  listStreamingTranslations,
  listTaskModels,
  onStreamingSessionEnded,
  onStreamingError,
  onStreamingPartial,
  onStreamingSources,
  onStreamingWindow,
  openStreamingSession,
  renameStreamingSession,
  revertStreamingPrettify,
  saveTextDialog,
  setSetting,
  setStreamingTranslationEnabled,
  startStreamingSession,
  stopStreamingSession,
  translateStreamingWindow,
  type StreamingMfu,
  type CloudProviderConfiguration,
  type StreamingSessionSummary,
  type StreamingTranslationTargetLanguage,
  type StreamingWindow,
} from "./ipc";
import {
  hasStreamingTranslations,
  renderStreamingPaired,
  STREAMING_TARGET_LANGUAGE_NAMES,
} from "./export";
import { AppLogo, Icon } from "./Icon";
import { ActionIcon } from "./ActionIcon";
import { CopyButton } from "./CopyButton";
import { ModeToggle } from "./ModeToggle";
import { ToggleSwitch } from "./ToggleSwitch";
import { formatElapsedClock } from "./format";
import { computeWordDiff } from "./diff";
import { groupWindowsIntoParagraphs } from "./paragraphs";
import { StreamingSessionRow } from "./StreamingSessionRow";
import {
  fileNameFor,
  formatClockTime,
  plainTranscript,
  sourcesLabel,
  toMarkdown,
  type TranslationEntry,
  upsertWindow,
  windowText,
  windowTranslationDisplay,
} from "./streamingText";
import {
  resolveStreamingRowStatus,
  resolveStreamingWidgetStatus,
} from "./streamingStatus";

// WP-93: Live Translation's target-language options — the select's display
// names and the split grid's target-column header (uppercased). Shared with
// the paired export renderer so the two cannot drift.
const TARGET_LANGUAGE_NAMES = STREAMING_TARGET_LANGUAGE_NAMES;

// WP-103: rolling context — the up-to-2 immediately preceding windows'
// available translations, joined in order (skips a failed/unavailable one
// rather than blocking). `index` is a position in `windows`, not a
// `window_index` value.
function precedingWindowsContext(
  windows: StreamingWindow[],
  index: number,
  entries: Map<number, TranslationEntry>,
): string | undefined {
  const parts: string[] = [];
  for (let i = Math.max(0, index - 2); i < index; i++) {
    const w = windows[i];
    if (!w) continue;
    const entry = entries.get(w.window_index);
    if (!entry) continue;
    if (entry.status !== "done" && entry.status !== "mirrored") continue;
    if (entry.translatedText === undefined) continue;
    parts.push(entry.translatedText);
  }
  return parts.length > 0 ? parts.join(" ") : undefined;
}

export function StreamingView({
  onClose,
  onOpenSettings,
  settingsOpen = false,
  onStreamingActivityChange,
}: {
  onClose: () => void;
  onOpenSettings: () => void;
  settingsOpen?: boolean;
  onStreamingActivityChange?: (active: boolean) => void;
}) {
  const [sessions, setSessions] = useState<StreamingSessionSummary[]>([]);
  const [sessionSearch, setSessionSearch] = useState("");
  const [activeId, setActiveId] = useState<number | null>(null);
  const [activeTitle, setActiveTitle] = useState<string>("Streaming Session");
  const [windows, setWindows] = useState<StreamingWindow[]>([]);
  const [isRunning, setIsRunning] = useState(false);
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
  const [transcriptionEngine, setTranscriptionEngine] = useState<
    "local" | "cloud"
  >("local");
  // The engine of the currently opened persisted session. Its configuration
  // is immutable; selecting another engine deliberately starts a new session.
  const [activeSessionEngine, setActiveSessionEngine] = useState<
    "local" | "cloud" | null
  >(null);
  const [cloudConfiguration, setCloudConfiguration] =
    useState<CloudProviderConfiguration | null>(null);
  const cloudConfigurationRequestRef = useRef(0);
  const [isStartPending, setIsStartPending] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    onStreamingActivityChange?.(isRunning || isStartPending);
  }, [isRunning, isStartPending, onStreamingActivityChange]);
  const [elapsed, setElapsed] = useState(0);
  const startTimeRef = useRef<number | null>(null);
  const [craftingId, setCraftingId] = useState<number | null>(null);
  const [mfu, setMfu] = useState<StreamingMfu | null>(null);
  const [craftFailed, setCraftFailed] = useState(false);
  const [prettifyingId, setPrettifyingId] = useState<number | null>(null);
  const [prettifyFailed, setPrettifyFailed] = useState(false);
  // WP-96: view-only visibility of the MFU (summary) panel, persisted under
  // its own settings key independently of Meeting's. Defaults ON; a settings
  // read/write failure keeps it ON without a blocking error (see the
  // getSettings effect and handleToggleMfuPanel below).
  const [mfuPanelVisible, setMfuPanelVisible] = useState(true);
  const [prettifiedText, setPrettifiedText] = useState<string | null>(null);
  const [pendingPrettify, setPendingPrettify] = useState<{
    original: string;
    cleaned: string;
  } | null>(null);
  // In-app modals (matching App.tsx's Meeting rename/delete pattern) rather
  // than window.prompt/window.confirm — Tauri's WKWebView doesn't reliably
  // wire up the native JS dialog delegate, so those silently no-op instead
  // of showing anything.
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
  // WP-93/WP-103: Live Translation — switch state, locked-while-on target
  // language (default Russian — English -> Russian is the primary use case,
  // never persisted), and per-*window* translation status keyed by
  // window_index (WP-103 moved this off paragraph_key). The queue itself
  // lives in refs (not state) since it's an implementation detail that never
  // renders directly.
  const [translationEnabled, setTranslationEnabled] = useState(false);
  const [targetLanguage, setTargetLanguage] =
    useState<StreamingTranslationTargetLanguage>("ru");
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
    { windowIndex: number; sourceText: string }[]
  >([]);
  const translationBusyRef = useRef(false);
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
  // Read after an await to avoid acting on a stale closure once the user has
  // switched sessions — plain state would still hold the id captured when
  // the async handler started.
  const activeIdRef = useRef<number | null>(null);
  useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);

  // Scoped to the open session, so switching sessions can't paint a stale
  // crafting/prettifying state onto the newly opened one.
  const isCraftingActive = craftingId !== null && craftingId === activeId;
  const isPrettifyingActive =
    prettifyingId !== null && prettifyingId === activeId;

  // Derived, not stored, so it can't drift from Start/Stop; isRunning wins
  // over busy so a Stop-in-flight still reads as On Air.
  const widgetStatus = isRunning
    ? "on-air"
    : busy
      ? "starting"
      : isCraftingActive
        ? "crafting"
        : isPrettifyingActive
          ? "prettifying"
          : craftFailed
            ? "mfu-failed"
            : prettifyFailed
              ? "prettify-failed"
              : "ready";
  const widget = resolveStreamingWidgetStatus(widgetStatus);
  const filteredSessions = useMemo(() => {
    const query = sessionSearch.trim().toLocaleLowerCase();
    if (query.length < 3) return sessions;
    return sessions.filter((session) =>
      session.title.toLocaleLowerCase().includes(query),
    );
  }, [sessionSearch, sessions]);

  // Recomputed from Date.now() each tick, not incremented, so a throttled
  // setInterval can't drift the displayed value. Shared by On Air, Crafting,
  // and Prettifying — mutually exclusive states (each is disabled while any
  // other is active).
  useEffect(() => {
    if (!isRunning && !isCraftingActive && !isPrettifyingActive) return;
    startTimeRef.current = Date.now();
    setElapsed(0);
    const id = setInterval(() => {
      setElapsed(Math.floor((Date.now() - startTimeRef.current!) / 1000));
    }, 1000);
    return () => clearInterval(id);
  }, [isRunning, isCraftingActive, isPrettifyingActive]);

  const refreshSessions = useCallback(async () => {
    setSessions(await listStreamingSessions());
  }, []);

  useEffect(() => {
    void refreshSessions();
  }, [refreshSessions]);

  useEffect(() => {
    getSettings()
      .then((s) => setMfuPanelVisible(s.mfu_panel_streaming ?? true))
      .catch(() => setMfuPanelVisible(true));
  }, []);

  const refreshCloudConfiguration = useCallback(async () => {
    const request = ++cloudConfigurationRequestRef.current;
    try {
      const configuration = await getCloudProviderConfig();
      if (request === cloudConfigurationRequestRef.current) {
        setCloudConfiguration(configuration);
      }
      return configuration;
    } catch {
      if (request === cloudConfigurationRequestRef.current) {
        setCloudConfiguration(null);
      }
      return null;
    }
  }, []);

  useEffect(() => {
    if (!settingsOpen) void refreshCloudConfiguration();
  }, [refreshCloudConfiguration, settingsOpen]);

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

  // View-only: never gates Craft MFU, Prettify, Start, or Stop. Persistence
  // is best-effort — a write failure leaves the switch exactly as the user
  // set it, with no blocking error.
  const handleToggleMfuPanel = useCallback((next: boolean) => {
    setMfuPanelVisible(next);
    void (async () => {
      try {
        await setSetting("mfu_panel_streaming", next ? "true" : "false");
      } catch {
        // Best-effort persistence: the switch already reflects `next`.
      }
    })();
  }, []);

  useEffect(() => {
    let unlistenWindow: (() => void) | undefined;
    let unlistenSources: (() => void) | undefined;
    let unlistenEnded: (() => void) | undefined;
    let unlistenPartial: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    let cancelled = false;

    void (async () => {
      const [w, s, e, p, er] = await Promise.all([
        onStreamingWindow((incoming) => {
          setActiveId((current) => {
            if (current === incoming.session_id) {
              setWindows((prev) => upsertWindow(prev, incoming));
              setPartialTranscript((partial) => {
                if (
                  partial === null ||
                  (incoming.item_id != null &&
                    incoming.item_id !== partial.itemId)
                ) {
                  return partial;
                }
                return null;
              });
            }
            return current;
          });
        }),
        onStreamingSources((incoming) => {
          setSources({
            mic: incoming.mic,
            system_audio: incoming.system_audio,
          });
        }),
        onStreamingSessionEnded(() => {
          setIsRunning(false);
          setPartialTranscript(null);
          void refreshSessions();
        }),
        onStreamingPartial((incoming) => {
          if (activeIdRef.current === incoming.session_id) {
            setPartialTranscript({
              itemId: incoming.item_id,
              text: incoming.text,
            });
          }
        }),
        onStreamingError((incoming) => {
          if (activeIdRef.current === incoming.session_id)
            setError(incoming.message);
        }),
      ]);
      if (cancelled) {
        w();
        s();
        e();
        p();
        er();
        return;
      }
      unlistenWindow = w;
      unlistenSources = s;
      unlistenEnded = e;
      unlistenPartial = p;
      unlistenError = er;
    })();

    return () => {
      cancelled = true;
      unlistenWindow?.();
      unlistenSources?.();
      unlistenEnded?.();
      unlistenPartial?.();
      unlistenError?.();
    };
  }, [refreshSessions]);

  // `resumeId` is the session to continue into, or null for a brand-new
  // one. LLM-result state (MFU/prettified/failed) is cleared either way —
  // only `windows` survives a resume. WP-101: Live Translation state is the
  // exception, reset only on an actual session-identity change (not when
  // resuming the session already open); `activeIdRef` stays current across
  // renders, unlike the `activeId` state this callback would otherwise
  // close over.
  const startSession = useCallback(
    async (resumeId: number | null, engine: "local" | "cloud") => {
      setError(null);
      setBusy(true);
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
        if (resumeId === null) setWindows([]);
        setSources(null);
        setPartialTranscript(null);
        setMfu(null);
        setCraftFailed(false);
        setPrettifiedText(null);
        setPrettifyFailed(false);
        setPendingPrettify(null);
        if (!isSameSessionResume) {
          commitTranslations(new Map());
          translationQueueRef.current = [];
          translationTokenRef.current += 1;
          persistedTranslationsRef.current = new Map();
          setPersistedReady(false); // WP-102: see handleOpen.
        }
        setIsRunning(true);
        await refreshSessions();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [refreshSessions],
  );

  // Reads as "continue what's on screen": resumes the currently open stopped
  // session, otherwise starts fresh — bound to the header's Start icon.
  const handleStart = useCallback(async () => {
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
    const engineChangedFromOpenedSession =
      activeSessionEngine !== null &&
      activeSessionEngine !== transcriptionEngine;
    const resumeId =
      activeId !== null && !isRunning && !engineChangedFromOpenedSession
        ? activeId
        : null;
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
    refreshCloudConfiguration,
    startSession,
    transcriptionEngine,
  ]);

  // Creates a brand-new, stopped session regardless of what's open. The
  // separate Start action is the only path that begins audio capture.
  const handleCreateNew = useCallback(async () => {
    setError(null);
    setBusy(true);
    try {
      const summary = await createStreamingSession();
      setActiveId(summary.id);
      setActiveTitle(summary.title);
      setActiveSessionEngine(null);
      setWindows([]);
      setIsRunning(false);
      setSources(null);
      setMfu(null);
      setCraftFailed(false);
      setPrettifiedText(null);
      setPrettifyFailed(false);
      setPendingPrettify(null);
      setTranslationEnabled(false);
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

  const handleStop = useCallback(async () => {
    setError(null);
    setBusy(true);
    try {
      await stopStreamingSession();
      // isRunning flips to false when streaming_session_ended fires, not
      // here — the backend, not this call returning, is the source of
      // truth for when the session actually finished.
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const handleOpen = useCallback(async (id: number) => {
    setError(null);
    try {
      const session = await openStreamingSession(id);
      setActiveId(id);
      setActiveTitle(session.title);
      setActiveSessionEngine(session.transcription_engine ?? null);
      if (session.transcription_engine) {
        setTranscriptionEngine(session.transcription_engine);
      }
      setWindows(session.windows);
      setIsRunning(false);
      setSources(null);
      setMfu(session.mfu ?? null);
      setCraftFailed(false);
      setPrettifiedText(session.prettified_text ?? null);
      setPrettifyFailed(false);
      setPendingPrettify(null);
      // WP-101: restore this session's own persisted choice instead of
      // always forcing it off — a session's Live Translation state now
      // survives reopening it (and an app restart), matching WP-96's
      // MFU-panel persistence. Absent (pre-WP-101 data) reads as off.
      setTranslationEnabled(session.translation_enabled ?? false);
      commitTranslations(new Map());
      translationQueueRef.current = [];
      translationTokenRef.current += 1;
      persistedTranslationsRef.current = new Map();
      // WP-102: pairs with the ref clear above so a `persistedReady` left
      // over `true` from a prior session can't race the reconcile effect
      // ahead of this session's own persisted-fetch effect.
      setPersistedReady(false);
    } catch (e) {
      setError(String(e));
    }
  }, []);

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
        setRenameError("Session label is required");
        return;
      }
      if (Array.from(title).length > 120) {
        setRenameError("Session label must be 120 characters or fewer");
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
        setCraftFailed(false);
        setPrettifiedText(null);
        setPrettifyFailed(false);
        setPendingPrettify(null);
        setTranslationEnabled(false);
        commitTranslations(new Map());
        translationQueueRef.current = [];
        translationTokenRef.current += 1;
        persistedTranslationsRef.current = new Map();
        setPersistedReady(false); // WP-102: see handleOpen.
        return null;
      });
      await refreshSessions();
      setDeleteTarget(null);
    } catch (e) {
      setError(String(e));
    }
  }, [activeId, deleteTarget, refreshSessions]);

  // Once accepted, the cleaned text is what gets copied/exported. Otherwise,
  // when Live Translation is on with at least one translation entry,
  // Copy/Export switch to the paired original+translation rendering
  // (WP-94); empty/off falls through to today's plain transcript unchanged.
  // Prettify and Live Translation are mutually exclusive (see
  // `prettifyDisabledByTranslation` below), so `prettifiedText` and
  // `translationEnabled` are never both truthy at once.
  const exportText =
    prettifiedText ??
    (translationEnabled && hasStreamingTranslations(translations)
      ? renderStreamingPaired(
          groupWindowsIntoParagraphs(windows),
          translations,
          targetLanguage,
        )
      : plainTranscript(windows));

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
    // A hidden panel is auto-revealed so the generated result is never left
    // behind it — the generation itself is unaffected either way.
    handleToggleMfuPanel(true);
    setError(null);
    setCraftFailed(false);
    setCraftingId(id);
    try {
      const session = await generateStreamingMfu(id);
      if (activeIdRef.current === id) {
        setMfu(session.mfu ?? null);
      }
    } catch (e) {
      if (activeIdRef.current === id) {
        setError(String(e));
        setCraftFailed(true);
      }
    } finally {
      setCraftingId((current) => (current === id ? null : current));
    }
  }, [activeId, handleToggleMfuPanel]);

  const handlePrettify = useCallback(async () => {
    // c8 ignore next -- the action is not rendered until an active session exists.
    if (activeId === null) return;
    const id = activeId;
    const original = plainTranscript(windows);
    setError(null);
    setPrettifyFailed(false);
    setPrettifyingId(id);
    try {
      const cleaned = await generateStreamingPrettify(id);
      if (activeIdRef.current === id) {
        setPendingPrettify({ original, cleaned });
      }
    } catch (e) {
      if (activeIdRef.current === id) {
        setError(String(e));
        setPrettifyFailed(true);
      }
    } finally {
      setPrettifyingId((current) => (current === id ? null : current));
    }
  }, [activeId, windows]);

  const handleAcceptPrettify = useCallback(async () => {
    // c8 ignore next -- Accept is only rendered while a review is pending.
    if (activeId === null || !pendingPrettify) return;
    const id = activeId;
    const text = pendingPrettify.cleaned;
    setError(null);
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
    try {
      const session = await revertStreamingPrettify(id);
      if (activeIdRef.current === id) {
        setPrettifiedText(session.prettified_text ?? null);
      }
    } catch (e) {
      if (activeIdRef.current === id) setError(String(e));
    }
  }, [activeId, prettifiedText]);

  // --- WP-93/WP-103: Live Translation --------------------------------------

  // Runs the queue's next item, if any, honoring the single-flight
  // constraint (`translationBusyRef`). Context (WP-103) is read fresh here
  // at dequeue time from `windowsRef`/`translationsRef`, not snapshotted at
  // enqueue time — required for the 2-window bootstrap pair, where window 1
  // is enqueued before window 0 has actually translated.
  function runTranslationQueue() {
    if (translationBusyRef.current) return;
    const item = translationQueueRef.current.shift();
    if (!item) return;
    const sessionId = activeIdRef.current;
    if (sessionId === null) return;
    const lang = targetLanguage;
    const token = translationTokenRef.current;
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
    const queue = translationQueueRef.current;
    const index = queue.findIndex((entry) => entry.windowIndex === windowIndex);
    if (index >= 0) {
      queue[index] = { windowIndex, sourceText };
    } else {
      queue.push({ windowIndex, sourceText });
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

  // WP-103: the retry affordance stays at the paragraph level (one button,
  // not one per window) — re-enqueues every currently-FAILED window within
  // that paragraph, leaving its done/mirrored/pending/translating siblings
  // untouched. Context for each retried window is recomputed at retry time
  // (like any other enqueue) by `runTranslationQueue`'s dequeue-time lookup,
  // from the windows' current state — not from whatever it was originally.
  const handleRetryTranslation = useCallback(
    (paragraphKey: number) => {
      const paragraphs = groupWindowsIntoParagraphs(windows);
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
    [windows, translations],
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
  // groupWindowsIntoParagraphs' on-screen use below). Nothing is enqueued
  // until the session has 2+ windows; a failed entry whose source text still
  // matches is left alone — retry is manual only.
  useEffect(() => {
    if (!translationEnabled || activeId === null || !persistedReady) return;
    if (windows.length < 2) return;
    const next = new Map(translationsRef.current);
    let changed = false;
    const toEnqueue: { windowIndex: number; sourceText: string }[] = [];
    for (const w of windows) {
      const sourceText = windowText(w);
      const existing = next.get(w.window_index);
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

  const headerLocked = isRunning || isStartPending;
  const translationDisabledReason = headerLocked
    ? "Streaming controls are unavailable while capture is active."
    : !llmModelReady
      ? "Live Translation needs a downloaded language model."
      : prettifiedText !== null
        ? "Turn off Prettify to use Live Translation."
        : pendingPrettify !== null
          ? "Finish or cancel the Prettify review to use Live Translation."
          : null;
  const prettifyDisabledByTranslation = translationEnabled;

  // --- end WP-93 -----------------------------------------------------------

  const hasText = windows.some((w) => windowText(w).length > 0);
  // Craft/Prettify need real decoded content, unlike Copy/Export's hasText —
  // a session with only fail-open windows has hasText=true (from the
  // "[unavailable]" placeholder) but nothing for the LLM to work from.
  const hasCraftableText = windows.some(
    (w) => w.outcome_ok && w.text.length > 0,
  );
  const canPrettify =
    headerLocked ||
    !hasCraftableText ||
    isCraftingActive ||
    isPrettifyingActive;

  const durationLabel =
    windows.length === 0
      ? "—"
      : formatClockTime(windows[windows.length - 1].end_ms);

  const activeBusy = isRunning || isCraftingActive || isPrettifyingActive;
  const selectedCloudProvider = cloudConfiguration?.providers.find(
    (provider) => provider.id === cloudConfiguration.selected_provider,
  );

  return (
    <div className="app streaming-view">
      <header className="wp-header" data-tauri-drag-region="deep">
        <div className="wp-header-lead">
          <div className="wp-header-left">
            <span
              className="wp-traffic-space"
              aria-hidden="true"
              data-tauri-drag-region
            />
            <AppLogo size={28} />
            <div className="wp-action-group">
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="Toggle sidebar"
                aria-pressed={sidebarOpen}
                onClick={() => setSidebarOpen((v) => !v)}
              >
                <Icon name="panel-left" size={18} />
              </button>
              <span className="wp-sep" />
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="New streaming session"
                title="New streaming session"
                onClick={() => void handleCreateNew()}
                disabled={busy || isRunning}
              >
                <Icon name="plus" size={18} />
              </button>
              <span className="wp-sep" />
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="Settings"
                onClick={onOpenSettings}
              >
                <Icon name="settings" size={18} />
              </button>
            </div>
          </div>

          <div className="wp-title-group">
            <h1 className="wp-title">{activeTitle}</h1>
            <button
              type="button"
              className="wp-icon-btn wp-icon-btn--ghost"
              aria-label="Rename session"
              onClick={() =>
                activeId !== null && openRename(activeId, activeTitle)
              }
              disabled={activeId === null || activeBusy}
            >
              <Icon name="pencil" size={14} />
            </button>
            <button
              type="button"
              className="wp-icon-btn wp-icon-btn--ghost"
              aria-label="Delete session"
              onClick={() =>
                activeId !== null && openDelete(activeId, activeTitle)
              }
              disabled={activeId === null || activeBusy}
            >
              <Icon name="trash-2" size={14} />
            </button>
          </div>
        </div>

        <div className="wp-header-right">
          <div className="wp-status" role="status">
            <Icon
              name={widget.icon}
              size={14}
              className={
                widget.spinning
                  ? `wp-spin wp-tone--${widget.tone} wp-status--${widget.statusKey}`
                  : `wp-tone--${widget.tone} wp-status--${widget.statusKey}`
              }
            />
            <span
              className={`wp-status-label wp-tone--${widget.tone} wp-status--${widget.statusKey}`}
            >
              {widget.label}
            </span>
            {widget.showTimer && (
              // aria-hidden: a role="status" live region re-announces on
              // every accessible-tree change: without this, the ticking
              // timer would spam a screen reader once per second.
              <span className="wp-status-timer" aria-hidden="true">
                {formatElapsedClock(elapsed)}
              </span>
            )}
          </div>

          <div className="wp-action-group">
            <ActionIcon
              icon="play"
              label={
                activeId !== null && !isRunning && windows.length > 0
                  ? "Resume"
                  : "Start"
              }
              onClick={() => void handleStart()}
              disabled={busy || isRunning}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="square"
              label="Stop"
              onClick={() => void handleStop()}
              disabled={busy || !isRunning}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="sparkles"
              label="Craft MFU"
              accent
              onClick={() => void handleCraft()}
              disabled={canPrettify}
            />
            <span className="wp-sep" />
            <CopyButton
              text={exportText}
              resetKey={activeId}
              onError={setError}
              onCopied={() => setError(null)}
              disabled={!hasText}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="download"
              label="Export as Markdown"
              onClick={() => void handleExport()}
              disabled={!hasText}
            />
            <span className="wp-sep" />
            <ActionIcon
              icon="trash-2"
              label="Delete active session"
              onClick={() =>
                activeId !== null && openDelete(activeId, activeTitle)
              }
              disabled={activeId === null || isRunning}
            />
          </div>
        </div>
      </header>

      <div className="wp-info-bar">
        <div className="wp-info-left">
          <span className="wp-info-label">Audio Source:</span>
          <span className="wp-file-chip">
            <Icon name="mic" size={14} />
            {sources ? sourcesLabel(sources) : "No audio source"}
          </span>
        </div>
        <div className="wp-info-right">
          <span className="wp-info-meta">
            <Icon name="globe" size={14} />
            Auto-detect (EN/RU/TR)
          </span>
          <span className="wp-info-meta">{durationLabel}</span>
        </div>
      </div>

      <div className="wp-main">
        {sidebarOpen && (
          <aside className="wp-sidebar">
            <ModeToggle
              mode="streaming"
              onSelectMeeting={onClose}
              onSelectStreaming={() => {}}
            />
            <div className="wp-search">
              <Icon name="search" size={16} />
              <input
                type="search"
                className="wp-search-input"
                placeholder="Search sessions..."
                aria-label="Search sessions"
                value={sessionSearch}
                onChange={(event) => setSessionSearch(event.target.value)}
              />
            </div>

            {sessions.length === 0 ? (
              <p className="wp-info-muted">No sessions yet</p>
            ) : filteredSessions.length === 0 ? (
              <p className="wp-info-muted">No matches</p>
            ) : (
              <ul className="wp-meeting-list" role="list">
                {filteredSessions.map((s) => (
                  <StreamingSessionRow
                    key={s.id}
                    title={s.title}
                    when={new Date(s.created_at_ms).toLocaleDateString()}
                    dur={formatClockTime(
                      Math.max(0, s.updated_at_ms - s.created_at_ms),
                    )}
                    status={
                      s.id === activeId
                        ? widget
                        : resolveStreamingRowStatus(s.status)
                    }
                    selected={activeId === s.id}
                    onSelect={() => void handleOpen(s.id)}
                    onRename={() => openRename(s.id, s.title)}
                    onDelete={() => openDelete(s.id, s.title)}
                  />
                ))}
              </ul>
            )}
          </aside>
        )}

        <section className="wp-workspace">
          <div className="wp-transcript-panel wp-transcript-panel--streaming">
            <div
              className={
                headerLocked
                  ? "wp-transcript-header wp-transcript-header--locked"
                  : "wp-transcript-header"
              }
            >
              <div className="wp-transcript-title-group">
                <h2 className="wp-transcript-title">Live Transcript</h2>
                <div
                  className="wp-ai-engine-toggle"
                  aria-label="Transcription engine"
                >
                  <span className="wp-ai-engine-label">AI</span>
                  <button
                    type="button"
                    className={
                      transcriptionEngine === "local"
                        ? "wp-engine-icon-button wp-engine-icon-button--active"
                        : "wp-engine-icon-button"
                    }
                    aria-label="Use local transcription"
                    aria-pressed={transcriptionEngine === "local"}
                    title="Local transcription"
                    onClick={() => {
                      setError(null);
                      setTranscriptionEngine("local");
                    }}
                    disabled={headerLocked}
                  >
                    <Icon name="cpu" size={15} />
                  </button>
                  <button
                    type="button"
                    className={
                      transcriptionEngine === "cloud"
                        ? "wp-engine-icon-button wp-engine-icon-button--active"
                        : "wp-engine-icon-button"
                    }
                    aria-label="Use cloud transcription"
                    aria-pressed={transcriptionEngine === "cloud"}
                    title="Cloud transcription"
                    onClick={() => {
                      setError(null);
                      setTranscriptionEngine("cloud");
                    }}
                    disabled={headerLocked}
                  >
                    <Icon name="cloud" size={15} />
                  </button>
                </div>
              </div>
              <div className="wp-translation-control">
                <Icon name="languages" size={15} />
                <ToggleSwitch
                  checked={translationEnabled}
                  onChange={handleToggleTranslation}
                  label="Live Translation"
                  disabled={translationDisabledReason !== null}
                  disabledReason={translationDisabledReason ?? undefined}
                />
                <select
                  className="wp-translation-lang-select"
                  aria-label="Live Translation target language"
                  value={targetLanguage}
                  disabled={translationEnabled || headerLocked}
                  title={
                    headerLocked
                      ? "Streaming controls are unavailable while capture is active."
                      : translationEnabled
                        ? "Turn off Live Translation to change the target language."
                        : "Target language"
                  }
                  onChange={(event) =>
                    setTargetLanguage(
                      event.target.value as StreamingTranslationTargetLanguage,
                    )
                  }
                >
                  <option value="en">English</option>
                  <option value="ru">Русский</option>
                </select>
              </div>
              <div className="wp-transcript-actions">
                <Icon name="pencil" size={14} />
                <span className="wp-transcript-editable-label">Editable</span>
                <span className="wp-sep" />
                {pendingPrettify && (
                  <>
                    <button
                      type="button"
                      className="wp-icon-btn"
                      aria-label="Accept Prettify"
                      title="Accept"
                      onClick={() => void handleAcceptPrettify()}
                      disabled={headerLocked}
                    >
                      <Icon
                        name="check"
                        size={15}
                        className="wp-tone--finished"
                      />
                    </button>
                    <button
                      type="button"
                      className="wp-icon-btn"
                      aria-label="Cancel Prettify"
                      title="Cancel"
                      onClick={handleCancelPrettify}
                      disabled={headerLocked}
                    >
                      <Icon name="x" size={15} />
                    </button>
                  </>
                )}
                {!pendingPrettify && prettifiedText !== null && (
                  <button
                    type="button"
                    className="wp-icon-btn"
                    aria-label="Cancel Prettify"
                    title="Revert to original transcript"
                    onClick={() => void handleRevertPrettify()}
                    disabled={headerLocked}
                  >
                    <Icon name="x" size={15} />
                  </button>
                )}
                <button
                  type="button"
                  className="wp-icon-btn wp-icon-btn--accent"
                  aria-label="Prettify transcript"
                  title={
                    prettifyDisabledByTranslation
                      ? "Turn off Live Translation to use Prettify."
                      : "Prettify transcript"
                  }
                  onClick={() => void handlePrettify()}
                  disabled={
                    canPrettify ||
                    pendingPrettify !== null ||
                    prettifyDisabledByTranslation
                  }
                >
                  <Icon name="wand-sparkles" size={15} />
                </button>
                <span className="wp-sep" />
                <span className="wp-mfu-toggle-label">MFU</span>
                <ToggleSwitch
                  checked={mfuPanelVisible}
                  onChange={handleToggleMfuPanel}
                  label="MFU panel"
                  disabled={headerLocked}
                  disabledReason="Streaming controls are unavailable while capture is active."
                />
              </div>
            </div>
            <div className="wp-separator" />

            {transcriptionEngine === "cloud" && (
              <div className="wp-cloud-notice" role="status">
                <Icon name="cloud-alert" size={15} />
                <span>
                  Cloud transcription sends live audio to{" "}
                  {selectedCloudProvider?.name ?? "your selected provider"}.
                  Usage is billed to your account.
                </span>
              </div>
            )}

            <div className="wp-transcript-content wp-transcript-content--inset">
              {error && (
                <div className="wp-notice wp-notice--error" role="alert">
                  {error}
                </div>
              )}
              {isRunning && partialTranscript && (
                <p className="wp-streaming-partial" role="status">
                  {partialTranscript.text}
                </p>
              )}

              {windows.length === 0 ? (
                <div className="wp-empty">
                  <p>
                    {isRunning
                      ? "Listening…"
                      : "Start a session, or open one from the list."}
                  </p>
                </div>
              ) : pendingPrettify ? (
                <div className="streaming-transcript-text">
                  {computeWordDiff(
                    pendingPrettify.original,
                    pendingPrettify.cleaned,
                  ).map((span, i) => {
                    // <del>/<ins> so a screen reader announces the change,
                    // not just strikethrough/color a sighted user sees.
                    if (span.type === "del") {
                      return (
                        <del key={i} className="diff-del">
                          {span.text}
                        </del>
                      );
                    }
                    if (span.type === "add") {
                      return (
                        <ins key={i} className="diff-add">
                          {span.text}
                        </ins>
                      );
                    }
                    return <span key={i}>{span.text}</span>;
                  })}
                </div>
              ) : prettifiedText !== null ? (
                <div className="streaming-transcript-text">
                  {prettifiedText}
                </div>
              ) : translationEnabled ? (
                <div className="wp-translation-grid">
                  <div className="wp-translation-columns wp-translation-columns--header">
                    <div className="wp-translation-col">
                      <span className="wp-translation-col-label">
                        ORIGINAL · AUTO-DETECTED
                      </span>
                    </div>
                    <div className="wp-translation-col-divider" />
                    <div className="wp-translation-col">
                      <span className="wp-translation-col-label">
                        {TARGET_LANGUAGE_NAMES[targetLanguage].toUpperCase()}
                      </span>
                    </div>
                  </div>
                  {groupWindowsIntoParagraphs(windows).map((paragraph) => {
                    const key = paragraph[0].window_index;
                    const sourceText = plainTranscript(paragraph);
                    const lastWindow = paragraph[paragraph.length - 1];
                    // WP-103: retry stays a single paragraph-level affordance
                    // — shown whenever at least one of this paragraph's
                    // windows is currently failed (with its stored source
                    // text still matching, i.e. not stale).
                    const hasFailedWindow = paragraph.some((w) => {
                      const entry = translations.get(w.window_index);
                      return (
                        entry !== undefined &&
                        entry.sourceText === windowText(w) &&
                        entry.status === "failed"
                      );
                    });
                    return (
                      <div
                        key={key}
                        className="wp-translation-columns wp-translation-row"
                      >
                        <div className="wp-translation-col">
                          <div className="wp-translation-meta">
                            <span className="wp-translation-timestamp">
                              {formatClockTime(paragraph[0].start_ms)}–
                              {formatClockTime(lastWindow.end_ms)}
                            </span>
                            <span className="wp-translation-lang-tag">
                              {paragraph[0].language.toUpperCase()}
                            </span>
                          </div>
                          <p className="wp-translation-text">{sourceText}</p>
                        </div>
                        <div className="wp-translation-col-divider" />
                        <div className="wp-translation-col">
                          <div className="wp-translation-meta">
                            <span className="wp-translation-lang-tag">
                              {targetLanguage.toUpperCase()}
                            </span>
                          </div>
                          {/* WP-103: each window renders its own slice —
                              real text once done/mirrored, an inline
                              placeholder otherwise — so a paragraph with a
                              still-in-flight trailing window shows real text
                              for its finished windows and a placeholder only
                              for that tail, not a blank/all-placeholder
                              cell. */}
                          <p className="wp-translation-text">
                            {paragraph.map((w, i) => {
                              const display = windowTranslationDisplay(
                                w,
                                translations.get(w.window_index),
                              );
                              return (
                                <span key={w.window_index}>
                                  {display.kind === "text" ? (
                                    <span
                                      className={
                                        display.mirrored
                                          ? "wp-translation-text--mirrored"
                                          : undefined
                                      }
                                    >
                                      {display.text}
                                    </span>
                                  ) : display.kind === "translating" ? (
                                    <span className="wp-translation-translating">
                                      <Icon
                                        name="loader"
                                        size={13}
                                        className="wp-spin"
                                      />
                                      <span>Translating…</span>
                                    </span>
                                  ) : display.kind === "failed" ? (
                                    <span>Translation failed</span>
                                  ) : (
                                    <span className="wp-translation-pending">
                                      Pending…
                                    </span>
                                  )}
                                  {i < paragraph.length - 1 ? " " : ""}
                                </span>
                              );
                            })}
                          </p>
                          {hasFailedWindow && (
                            <button
                              type="button"
                              className="wp-translation-retry"
                              onClick={() => handleRetryTranslation(key)}
                            >
                              <Icon name="rotate-ccw" size={13} />
                              Translation failed · Retry
                            </button>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <div className="streaming-transcript-text">
                  {groupWindowsIntoParagraphs(windows).map((paragraph) => (
                    <p
                      key={paragraph[0].window_index}
                      className="streaming-paragraph"
                    >
                      {paragraph.map((w) => (
                        <span
                          key={w.window_index}
                          className={
                            w.outcome_ok
                              ? "streaming-window"
                              : "streaming-window streaming-window--failed"
                          }
                          title={`${formatClockTime(w.start_ms)}–${formatClockTime(w.end_ms)} (${w.language})`}
                        >
                          {w.outcome_ok ? w.text : "[unavailable]"}{" "}
                        </span>
                      ))}
                    </p>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* MFU (summary) panel — hidden by the header switch (WP-96); the
              transcript panel above fills the freed width via its existing
              flex:1 in .wp-transcript-panel. */}
          {mfuPanelVisible && (
            <aside className="wp-mfu">
              {mfu ? (
                <div className="wp-mfu-content">
                  {mfu.summary && (
                    <section className="wp-mfu-section">
                      <h3 className="wp-mfu-heading">Summary</h3>
                      <p className="wp-mfu-text">{mfu.summary}</p>
                    </section>
                  )}
                  {mfu.decisions && (
                    <section className="wp-mfu-section">
                      <h3 className="wp-mfu-heading">Decisions</h3>
                      <p className="wp-mfu-text">{mfu.decisions}</p>
                    </section>
                  )}
                  {mfu.action_items && (
                    <section className="wp-mfu-section">
                      <h3 className="wp-mfu-heading">Action Items</h3>
                      <p className="wp-mfu-text">{mfu.action_items}</p>
                    </section>
                  )}
                  {mfu.open_questions && (
                    <section className="wp-mfu-section">
                      <h3 className="wp-mfu-heading">Open Questions</h3>
                      <p className="wp-mfu-text">{mfu.open_questions}</p>
                    </section>
                  )}
                  {mfu.participants && (
                    <section className="wp-mfu-section">
                      <h3 className="wp-mfu-heading">Participants</h3>
                      <p className="wp-mfu-text">{mfu.participants}</p>
                    </section>
                  )}
                </div>
              ) : (
                <div className="wp-mfu-placeholder">
                  <div className="wp-mfu-icon-frame">
                    <Icon name="sparkles" size={32} />
                  </div>
                  <p className="wp-mfu-title">Run MFU Craft</p>
                  <p className="wp-mfu-subtitle">
                    Generate summary, decisions, action items, and open
                    questions.
                  </p>
                </div>
              )}
            </aside>
          )}
        </section>
      </div>

      {renameTarget && (
        <div className="modal-overlay">
          <form
            className="modal-panel confirm-modal"
            role="dialog"
            aria-modal="true"
            aria-label="Rename session"
            onSubmit={submitRename}
            onKeyDown={(event) => {
              if (event.key === "Escape") closeRename();
            }}
          >
            <div className="modal-header">
              <span className="modal-title">Rename session</span>
            </div>
            <label htmlFor="streaming-session-label">Session label</label>
            <input
              id="streaming-session-label"
              type="text"
              value={renameDraft}
              autoFocus
              onFocus={(event) => event.currentTarget.select()}
              onChange={(event) => {
                setRenameDraft(event.target.value);
                setRenameError(null);
              }}
              aria-invalid={renameError ? true : undefined}
            />
            {renameError && <p role="alert">{renameError}</p>}
            <div className="confirm-actions">
              <button type="button" onClick={closeRename}>
                Cancel
              </button>
              <button type="submit">Save</button>
            </div>
          </form>
        </div>
      )}

      {deleteTarget && (
        <div className="modal-overlay">
          <div
            className="modal-panel confirm-modal"
            role="alertdialog"
            aria-modal="true"
            aria-label={`Delete ${deleteTarget.title}`}
            onKeyDown={(event) => {
              if (event.key === "Escape") setDeleteTarget(null);
            }}
          >
            <div className="modal-header">
              <span className="modal-title">Delete {deleteTarget.title}?</span>
            </div>
            <p className="confirm-warning">
              This permanently removes the session and its transcript.
            </p>
            <div className="confirm-actions">
              <button type="button" onClick={() => setDeleteTarget(null)}>
                Cancel
              </button>
              <button type="button" onClick={() => void confirmDelete()}>
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
