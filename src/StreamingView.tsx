import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  acceptStreamingPrettify,
  createStreamingSession,
  deleteStreamingSession,
  generateStreamingMfu,
  generateStreamingPrettify,
  getSettings,
  listStreamingSessions,
  listStreamingTranslations,
  listTaskModels,
  onStreamingSessionEnded,
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
  translateStreamingParagraph,
  type StreamingMfu,
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
import { groupWindowsIntoParagraphs, isParagraphClosed } from "./paragraphs";
import { StreamingSessionRow } from "./StreamingSessionRow";
import {
  fileNameFor,
  formatClockTime,
  plainTranscript,
  sourcesLabel,
  toMarkdown,
  upsertWindow,
  windowText,
} from "./streamingText";
import {
  resolveStreamingRowStatus,
  resolveStreamingWidgetStatus,
} from "./streamingStatus";

// WP-93: Live Translation's target-language options — the select's display
// names and the split grid's target-column header (uppercased). Shared with
// the paired export renderer so the two cannot drift.
const TARGET_LANGUAGE_NAMES = STREAMING_TARGET_LANGUAGE_NAMES;

type TranslationStatus =
  "pending" | "translating" | "done" | "mirrored" | "failed";

/** One paragraph's Live Translation state, keyed by `paragraph_key` (the
 * `window_index` of the paragraph's first window). `sourceText` is the
 * paragraph text this entry was produced from — comparing it against the
 * paragraph's *current* text is how a stale entry (the paragraph's windows
 * changed since) is detected and replaced. */
interface TranslationEntry {
  status: TranslationStatus;
  sourceText: string;
  translatedText?: string;
}

export function StreamingView({
  onClose,
  onOpenSettings,
}: {
  onClose: () => void;
  onOpenSettings: () => void;
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
  const [busy, setBusy] = useState(false);
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
  // WP-93: Live Translation — switch state, locked-while-on target language
  // (default Russian — English -> Russian is the primary use case, never
  // persisted), and per-paragraph translation status keyed by paragraph_key.
  // The queue itself lives in refs (not state) since it's an implementation
  // detail that never renders directly.
  const [translationEnabled, setTranslationEnabled] = useState(false);
  const [targetLanguage, setTargetLanguage] =
    useState<StreamingTranslationTargetLanguage>("ru");
  const [translations, setTranslations] = useState<
    Map<number, TranslationEntry>
  >(new Map());
  const [llmModelReady, setLlmModelReady] = useState(false);
  // `context` (WP-100) is the immediately preceding paragraph's translated
  // text at the moment this paragraph was enqueued — a snapshot, not a live
  // reference, so a later status change on the previous paragraph can't
  // retroactively alter a call already queued.
  const translationQueueRef = useRef<
    { key: number; sourceText: string; context?: string }[]
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
  // result already covers a paragraph.
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
    let cancelled = false;

    void (async () => {
      const [w, s, e] = await Promise.all([
        onStreamingWindow((incoming) => {
          setActiveId((current) => {
            if (current === incoming.session_id) {
              setWindows((prev) => upsertWindow(prev, incoming));
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
          void refreshSessions();
        }),
      ]);
      if (cancelled) {
        w();
        s();
        e();
        return;
      }
      unlistenWindow = w;
      unlistenSources = s;
      unlistenEnded = e;
    })();

    return () => {
      cancelled = true;
      unlistenWindow?.();
      unlistenSources?.();
      unlistenEnded?.();
    };
  }, [refreshSessions]);

  // `resumeId` is the session to continue capturing into, or null for a
  // brand-new one. Either way, any LLM-result state (MFU/prettified/failed
  // flags) is stale the moment new audio starts arriving, so it's cleared in
  // both cases — only `windows` survives a resume, since preserving the
  // session's transcript-so-far is the whole point of resuming into it.
  //
  // WP-101: Live Translation state (the switch, its map, and the queue/
  // token/persisted refs) is the one exception — it's reset only when the
  // session identity is actually changing (a brand-new session, or resuming
  // a *different* past session), not when resuming the session that's
  // already open. `activeIdRef` is always current across renders, unlike the
  // `activeId` state this callback would otherwise close over.
  const startSession = useCallback(
    async (resumeId: number | null) => {
      setError(null);
      setBusy(true);
      try {
        const isSameSessionResume =
          resumeId !== null && resumeId === activeIdRef.current;
        const summary = await startStreamingSession(resumeId ?? undefined);
        setActiveId(summary.id);
        setActiveTitle(summary.title);
        if (resumeId === null) setWindows([]);
        setSources(null);
        setMfu(null);
        setCraftFailed(false);
        setPrettifiedText(null);
        setPrettifyFailed(false);
        setPendingPrettify(null);
        if (!isSameSessionResume) {
          setTranslationEnabled(false);
          setTranslations(new Map());
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
  const handleStart = useCallback(() => {
    const resumeId = activeId !== null && !isRunning ? activeId : null;
    return startSession(resumeId);
  }, [startSession, activeId, isRunning]);

  // Creates a brand-new, stopped session regardless of what's open. The
  // separate Start action is the only path that begins audio capture.
  const handleCreateNew = useCallback(async () => {
    setError(null);
    setBusy(true);
    try {
      const summary = await createStreamingSession();
      setActiveId(summary.id);
      setActiveTitle(summary.title);
      setWindows([]);
      setIsRunning(false);
      setSources(null);
      setMfu(null);
      setCraftFailed(false);
      setPrettifiedText(null);
      setPrettifyFailed(false);
      setPendingPrettify(null);
      setTranslationEnabled(false);
      setTranslations(new Map());
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
      setTranslations(new Map());
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
      setActiveId((current) => {
        if (current !== target.id) return current;
        setWindows([]);
        setMfu(null);
        setCraftFailed(false);
        setPrettifiedText(null);
        setPrettifyFailed(false);
        setPendingPrettify(null);
        setTranslationEnabled(false);
        setTranslations(new Map());
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
  }, [deleteTarget, refreshSessions]);

  // Once accepted, the cleaned text is what gets copied/exported — that's
  // the point of prettifying. Otherwise, when Live Translation is on and at
  // least one paragraph has a translation entry, Copy/Export switch to the
  // paired original+translation rendering (WP-94); an empty translations
  // map (off, or on with nothing recorded yet) falls through to today's
  // plain transcript unchanged, so that output stays byte-identical.
  // Mutual exclusion between Prettify and Live Translation (see
  // `prettifyDisabledByTranslation` below) means `prettifiedText` and
  // `translationEnabled` are never both truthy at once.
  const exportText =
    prettifiedText ??
    (translationEnabled && hasStreamingTranslations(translations)
      ? renderStreamingPaired(
          groupWindowsIntoParagraphs(windows).map((paragraph) => ({
            key: paragraph[0].window_index,
            sourceText: plainTranscript(paragraph),
          })),
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

  // --- WP-93: Live Translation --------------------------------------------

  // Runs the queue's next item, if any, honoring the single-flight
  // constraint (`translationBusyRef`). Reads `targetLanguage`/`translations`
  // from this render's closure — safe because the target language is locked
  // (the select is disabled) for the whole time a run can be in flight, and
  // every state write below re-derives from the freshest `prev` via the
  // functional setState form.
  function runTranslationQueue() {
    if (translationBusyRef.current) return;
    const item = translationQueueRef.current.shift();
    if (!item) return;
    const sessionId = activeIdRef.current;
    if (sessionId === null) return;
    const lang = targetLanguage;
    const token = translationTokenRef.current;
    translationBusyRef.current = true;
    setTranslations((prev) => {
      const next = new Map(prev);
      next.set(item.key, {
        status: "translating",
        sourceText: item.sourceText,
      });
      return next;
    });
    void translateStreamingParagraph(
      sessionId,
      item.key,
      lang,
      item.sourceText,
      item.context,
    )
      .then((text) => {
        if (translationTokenRef.current !== token) return;
        setTranslations((prev) => {
          const current = prev.get(item.key);
          if (!current || current.sourceText !== item.sourceText) return prev;
          const next = new Map(prev);
          next.set(item.key, {
            status: "done",
            sourceText: item.sourceText,
            translatedText: text,
          });
          return next;
        });
      })
      .catch(() => {
        if (translationTokenRef.current !== token) return;
        setTranslations((prev) => {
          const current = prev.get(item.key);
          if (!current || current.sourceText !== item.sourceText) return prev;
          const next = new Map(prev);
          next.set(item.key, { status: "failed", sourceText: item.sourceText });
          return next;
        });
      })
      .finally(() => {
        translationBusyRef.current = false;
        runTranslationQueue();
      });
  }

  // Upserts by key so a paragraph whose text changed again before its
  // earlier queued attempt started replaces the stale payload rather than
  // running twice.
  function enqueueTranslation(
    key: number,
    sourceText: string,
    context?: string,
  ) {
    const queue = translationQueueRef.current;
    const index = queue.findIndex((entry) => entry.key === key);
    if (index >= 0) {
      queue[index] = { key, sourceText, context };
    } else {
      queue.push({ key, sourceText, context });
    }
    runTranslationQueue();
  }

  // WP-100: the immediately preceding paragraph's already-translated text,
  // passed as ephemeral prompt context — never stored, only used to shape
  // the next model call. Only a genuinely target-language entry ("done" or
  // "mirrored") counts as usable context; no entry (first paragraph, or the
  // previous one hasn't closed yet) and a "pending"/"translating"/"failed"
  // entry all contribute nothing.
  function priorParagraphContext(
    paragraphs: { window_index: number }[][],
    index: number,
    entries: Map<number, TranslationEntry>,
  ): string | undefined {
    if (index <= 0) return undefined;
    const previousParagraph = paragraphs[index - 1];
    if (!previousParagraph || previousParagraph.length === 0) return undefined;
    const previousKey = previousParagraph[0].window_index;
    const entry = entries.get(previousKey);
    if (!entry) return undefined;
    if (entry.status !== "done" && entry.status !== "mirrored")
      return undefined;
    return entry.translatedText;
  }

  // View-only; gates nothing else. Clearing translations/queue on both
  // directions (not just OFF) means turning back ON always re-derives fresh
  // from the current windows + a fresh persisted-translations fetch, so a
  // translation from a previous target-language run can never be reused
  // under a new target language.
  //
  // Turning ON also resets `persistedReady` synchronously here, mirroring
  // the persisted-fetch effect below. Without this, a second activation in
  // the same session would have the reconcile effect run in the *same*
  // commit as this toggle, observing the previous activation's leftover
  // `persistedReady === true` (that effect only flips it back to false
  // asynchronously, one render later) while `persistedTranslationsRef` has
  // already been cleared above — so every closed paragraph would look
  // "not yet persisted" and get queued for a live call it doesn't need.
  const handleToggleTranslation = useCallback((next: boolean) => {
    setTranslationEnabled(next);
    setTranslations(new Map());
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

  const handleRetryTranslation = useCallback(
    (key: number) => {
      const paragraphs = groupWindowsIntoParagraphs(windows);
      const index = paragraphs.findIndex((p) => p[0].window_index === key);
      if (index === -1) return;
      const paragraph = paragraphs[index];
      const sourceText = plainTranscript(paragraph);
      const context = priorParagraphContext(paragraphs, index, translations);
      setTranslations((prev) => {
        const next = new Map(prev);
        next.set(key, { status: "pending", sourceText });
        return next;
      });
      enqueueTranslation(key, sourceText, context);
    },
    [windows, translations],
  );

  // Loads this session+target-language's persisted translations once per
  // "Live Translation On" so the reconcile effect below can reuse them
  // instead of re-running the model (WP-92's single-flight command makes
  // replaying a whole session's paragraphs on every toggle expensive).
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
          map.set(row.paragraph_key, {
            text: row.translated_text,
            sourceText: row.source_text,
          });
        }
      } catch {
        // Best-effort: proceed with nothing persisted — paragraphs are
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

  // Reconciles every *closed* paragraph — a later paragraph exists, capture
  // has stopped, or (WP-100) the still-open trailing paragraph itself
  // already satisfies isParagraphClosed on its own text — against its
  // translation entry: same-language paragraphs are mirrored, a matching
  // persisted row is reused, and anything else missing or stale (source
  // text changed) is (re-)enqueued, with the immediately preceding
  // paragraph's translation threaded in as context when that entry is
  // "done" or "mirrored". A failed entry whose source text still matches is
  // left alone — retry is manual only.
  useEffect(() => {
    if (!translationEnabled || activeId === null || !persistedReady) return;
    const paragraphs = groupWindowsIntoParagraphs(windows);
    const next = new Map(translations);
    let changed = false;
    // Collected instead of enqueued inline: enqueueTranslation kicks the
    // queue synchronously, which would apply its "translating" write before
    // the batched `next` map below (still holding "pending" for that key)
    // gets applied — clobbering it back to "pending". Enqueuing only after
    // `next` is committed keeps the two writes in the right order.
    const toEnqueue: { key: number; sourceText: string; context?: string }[] =
      [];
    for (let index = 0; index < paragraphs.length; index++) {
      const paragraph = paragraphs[index];
      const isLastWhileRunning = isRunning && index === paragraphs.length - 1;
      if (isLastWhileRunning && !isParagraphClosed(paragraph)) continue;
      const key = paragraph[0].window_index;
      const sourceText = plainTranscript(paragraph);
      const existing = next.get(key);
      const isTargetAlready = paragraph.every(
        (w) => w.language.toLowerCase() === targetLanguage,
      );
      if (isTargetAlready) {
        if (
          !existing ||
          existing.sourceText !== sourceText ||
          existing.status !== "mirrored"
        ) {
          next.set(key, {
            status: "mirrored",
            sourceText,
            translatedText: sourceText,
          });
          changed = true;
        }
        continue;
      }
      const persisted = persistedTranslationsRef.current.get(key);
      if (persisted && persisted.sourceText === sourceText) {
        if (
          !existing ||
          existing.sourceText !== sourceText ||
          existing.status !== "done" ||
          existing.translatedText !== persisted.text
        ) {
          next.set(key, {
            status: "done",
            sourceText,
            translatedText: persisted.text,
          });
          changed = true;
        }
        continue;
      }
      if (!existing || existing.sourceText !== sourceText) {
        next.set(key, { status: "pending", sourceText });
        changed = true;
        const context = priorParagraphContext(paragraphs, index, next);
        toEnqueue.push({ key, sourceText, context });
      }
    }
    if (changed) setTranslations(next);
    for (const item of toEnqueue) {
      enqueueTranslation(item.key, item.sourceText, item.context);
    }
    // `translations` intentionally omitted: read directly from this
    // render's closure to decide reuse without re-running this effect on
    // every status transition the queue itself writes (translating/done/
    // failed) — those don't change which paragraphs are closed or stale.
  }, [
    windows,
    translationEnabled,
    activeId,
    targetLanguage,
    isRunning,
    persistedReady,
  ]);

  const translationDisabledReason = !llmModelReady
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
    isRunning || !hasCraftableText || isCraftingActive || isPrettifyingActive;

  const windowCountLabel =
    windows.length === 0
      ? "No windows"
      : `${windows.length} window${windows.length === 1 ? "" : "s"}`;
  const durationLabel =
    windows.length === 0
      ? "—"
      : formatClockTime(windows[windows.length - 1].end_ms);

  const activeBusy = isRunning || isCraftingActive || isPrettifyingActive;

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
            <div className="wp-transcript-header">
              <div className="wp-transcript-title-group">
                <h2 className="wp-transcript-title">Live Transcript</h2>
                <span className="wp-transcript-meta">{windowCountLabel}</span>
              </div>
              <div className="wp-translation-control">
                <span className="wp-translation-label">Live Translation</span>
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
                  disabled={translationEnabled}
                  title={
                    translationEnabled
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
                />
              </div>
            </div>
            <div className="wp-separator" />

            <div className="wp-transcript-content">
              {error && (
                <div className="wp-notice wp-notice--error" role="alert">
                  {error}
                </div>
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
                    const entry = translations.get(key);
                    const status = entry?.status ?? "pending";
                    const sourceText = plainTranscript(paragraph);
                    const lastWindow = paragraph[paragraph.length - 1];
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
                          {status === "translating" ? (
                            <span className="wp-translation-translating">
                              <Icon
                                name="loader"
                                size={13}
                                className="wp-spin"
                              />
                              <span>Translating…</span>
                            </span>
                          ) : status === "mirrored" ? (
                            <p className="wp-translation-text wp-translation-text--mirrored">
                              {entry?.translatedText ?? sourceText}
                            </p>
                          ) : status === "done" ? (
                            <p className="wp-translation-text">
                              {entry?.translatedText ?? ""}
                            </p>
                          ) : status === "failed" ? (
                            <button
                              type="button"
                              className="wp-translation-retry"
                              onClick={() => handleRetryTranslation(key)}
                            >
                              <Icon name="rotate-ccw" size={13} />
                              Translation failed · Retry
                            </button>
                          ) : (
                            <span className="wp-translation-pending">
                              Pending…
                            </span>
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
