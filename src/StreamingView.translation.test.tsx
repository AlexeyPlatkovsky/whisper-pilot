import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StreamingView } from "./StreamingView";
import * as ipc from "./ipc";
import type {
  StreamingSession,
  StreamingSessionSummary,
  StreamingWindow,
  TaskModel,
} from "./ipc";

// WP-93/WP-103: the Live Translation header control (label + switch + locked
// target-language select) and the two-column paired-row transcript grid,
// mirroring the inline vi.mock idiom of StreamingView.test.tsx /
// StreamingView.mfuToggle.test.tsx. WP-103 rewrote translation from one call
// per *paragraph* to one call per *window*: window 0 translates immediately
// with no context, window 1 follows with window 0's translation, and every
// later window translates alone with a rolling up-to-2-window context — all
// through the same single-flight queue, strictly in increasing window_index
// order.

type Handler<T> = (payload: T) => void;

let windowHandler: Handler<
  ipc.StreamingWindow & { session_id: number }
> | null = null;
let partialHandler: Handler<ipc.StreamingPartial> | null = null;
let liveCaptureHandler: Handler<ipc.LiveCaptureSnapshot> | null = null;
let liveCaptureRevision = 0;

const { previewStreamingTranslationMock } = vi.hoisted(() => ({
  previewStreamingTranslationMock: vi.fn(),
}));

vi.mock("./ipc", () => ({
  listStreamingSessions: vi.fn(async () => []),
  openStreamingSession: vi.fn(),
  renameStreamingSession: vi.fn(),
  deleteStreamingSession: vi.fn(),
  clearStreamingSession: vi.fn(),
  createStreamingSession: vi.fn(),
  startStreamingSession: vi.fn(),
  stopStreamingSession: vi.fn(),
  generateStreamingMfu: vi.fn(),
  generateStreamingPrettify: vi.fn(),
  acceptStreamingPrettify: vi.fn(),
  revertStreamingPrettify: vi.fn(),
  translateStreamingWindow: vi.fn(async () => "Translated."),
  previewStreamingTranslation: previewStreamingTranslationMock,
  listStreamingTranslations: vi.fn(async () => []),
  setStreamingTranslationEnabled: vi.fn(),
  setStreamingTranslationTargetLanguage: vi.fn(),
  onStreamingWindow: vi.fn(async (handler: Handler<unknown>) => {
    windowHandler = handler as Handler<
      ipc.StreamingWindow & { session_id: number }
    >;
    return () => {
      windowHandler = null;
    };
  }),
  onStreamingSources: vi.fn(async () => () => {}),
  onStreamingSessionEnded: vi.fn(async () => () => {}),
  onStreamingPartial: vi.fn(async (handler: Handler<unknown>) => {
    partialHandler = handler as Handler<ipc.StreamingPartial>;
    return () => {
      partialHandler = null;
    };
  }),
  onStreamingError: vi.fn(async () => () => {}),
  getLiveCaptureSnapshot: vi.fn(async () => ({
    phase: "idle" as const,
    session_id: null,
    source: null,
    generation: 0,
    revision: 0,
    error: null,
  })),
  onLiveCaptureState: vi.fn(async (handler: Handler<unknown>) => {
    liveCaptureHandler = handler as Handler<ipc.LiveCaptureSnapshot>;
    return () => {
      liveCaptureHandler = null;
    };
  }),
  saveTextDialog: vi.fn(async () => null),
  getSettings: vi.fn(async () => ({
    theme: "system",
    ui_language: "en",
    active_model_diarization: "none",
    export_file_type: "plain_text",
    active_model_llm: "llm-mini",
  })),
  setSetting: vi.fn(),
  listTaskModels: vi.fn(async () => [LLM_MODEL_READY]),
  getCloudProviderConfig: vi.fn(async () => ({
    selected_provider: "deepgram",
    providers: [
      { id: "deepgram", name: "Deepgram", model: "Nova-3", configured: false },
      {
        id: "assemblyai",
        name: "AssemblyAI",
        model: "Universal-3.5 Pro",
        configured: false,
      },
      {
        id: "openai",
        name: "OpenAI",
        model: "GPT Transcribe",
        configured: false,
      },
    ],
  })),
}));

const LLM_MODEL_READY: TaskModel = {
  id: "llm-mini",
  task: "llm",
  label: "Mini LLM",
  downloaded: true,
  size_bytes: 1000,
  recommended: true,
};

const SESSION_A: StreamingSessionSummary = {
  id: 1,
  title: "Standup",
  created_at_ms: 100,
  updated_at_ms: 100,
  status: "stopped",
  translation_enabled: false,
};

const ACTIVE_SESSION_A: StreamingSessionSummary = {
  ...SESSION_A,
  status: "active",
};

function openedSession(
  overrides: Partial<StreamingSession> = {},
): StreamingSession {
  return {
    id: 1,
    title: "Standup",
    created_at_ms: 100,
    updated_at_ms: 100,
    status: "stopped",
    translation_enabled: false,
    windows: [],
    ...overrides,
  };
}

/** `count` windows of monotonically increasing index. Sentence-closed fixture
 * groups below make paragraph boundaries deterministic for the on-screen
 * display and paragraph-level retry affordance. Default language is "en" — the
 * mirror image of the "ru" target-language default, so windows built with no
 * override exercise real translation instead of the same-language mirror
 * path. */
function makeWindows(
  count: number,
  opts: {
    startIndex?: number;
    language?: string;
    prefix?: string;
    outcomeOk?: boolean;
  } = {},
): StreamingWindow[] {
  const {
    startIndex = 0,
    language = "en",
    prefix = "Слово",
    outcomeOk = true,
  } = opts;
  return Array.from({ length: count }, (_, i) => {
    const index = startIndex + i;
    return {
      window_index: index,
      start_ms: index * 1000,
      end_ms: index * 1000 + 900,
      text: `${prefix}${index}`,
      language,
      outcome_ok: outcomeOk,
    };
  });
}

function paragraphSourceText(windows: StreamingWindow[]): string {
  return windows.map((w) => w.text).join(" ");
}

// Two sentence-closed English paragraphs. Acoustic window count no longer
// creates a semantic paragraph boundary.
const PARAGRAPH_A = makeWindows(4, { startIndex: 0 }).map((window, index) =>
  index === 3 ? { ...window, text: `${window.text}.` } : window,
);
const PARAGRAPH_B = makeWindows(4, { startIndex: 4 }).map((window, index) =>
  index === 3 ? { ...window, text: `${window.text}.` } : window,
);
const SOURCE_A = paragraphSourceText(PARAGRAPH_A);
const SOURCE_B = paragraphSourceText(PARAGRAPH_B);
const TWO_PARAGRAPHS = [...PARAGRAPH_A, ...PARAGRAPH_B];

/** Reads a paragraph row's translated-column text by locating the row via
 * its (untouched, plain-text) original column and reading the translated
 * `<p className="wp-translation-text">`'s `textContent` directly. Needed
 * instead of `screen.findByText` because that cell's content is nested
 * inside per-window `<span>`s (WP-103) — dom-testing-library's default text
 * matcher only reads an element's own direct text-node children, not
 * descendant elements' text, so it would never match the wrapping `<p>`. */
async function expectTranslatedCellText(sourceText: string, expected: string) {
  await waitFor(() => {
    const rows = Array.from(document.querySelectorAll(".wp-translation-row"));
    const row = rows.find((r) =>
      r.querySelector(".wp-translation-col")?.textContent?.includes(sourceText),
    );
    const cols = row?.querySelectorAll(".wp-translation-col");
    const cell = cols?.[1]?.querySelector(".wp-translation-text");
    expect(cell?.textContent).toBe(expected);
  });
}

function flush(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function openSessionWithWindows(
  user: ReturnType<typeof userEvent.setup>,
  windows: StreamingWindow[],
  overrides: Partial<StreamingSession> = {},
) {
  vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
  vi.mocked(ipc.openStreamingSession).mockResolvedValue(
    openedSession({ windows, ...overrides }),
  );
  render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
  await user.click(await screen.findByText("Standup"));
  await screen.findByRole("switch", { name: "Live Translation" });
}

// Waits past the async LLM-readiness fetch so callers that click/focus the
// switch don't race its initial (readiness-pending) disabled state.
async function findTranslationSwitch() {
  const toggle = await screen.findByRole("switch", {
    name: "Live Translation",
  });
  await waitFor(() => expect(toggle).not.toBeDisabled());
  return toggle;
}

// For tests asserting the switch IS disabled — no readiness wait, since the
// disabled state itself is what's under test.
async function findDisabledTranslationSwitch() {
  return screen.findByRole("switch", { name: "Live Translation" });
}

function findTargetLanguageSelect() {
  return screen.getByRole("combobox", {
    name: "Live Translation target language",
  });
}

/** Starts a running session and feeds it `windows` one by one through the
 * mocked `onStreamingWindow` handler — mirrors how live capture actually
 * delivers windows, one at a time, unlike `openSessionWithWindows`'s
 * already-fully-populated stopped session. */
async function startRunningSessionWithWindows(
  user: ReturnType<typeof userEvent.setup>,
  windows: StreamingWindow[],
) {
  vi.mocked(ipc.startStreamingSession).mockResolvedValue(ACTIVE_SESSION_A);
  render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
  const toggle = await findTranslationSwitch();
  await user.click(toggle);
  await user.click(await screen.findByRole("button", { name: "Start" }));
  liveCaptureRevision += 1;
  act(() => {
    liveCaptureHandler!({
      phase: "capturing",
      session_id: ACTIVE_SESSION_A.id,
      source: "streaming",
      generation: 1,
      revision: liveCaptureRevision,
      error: null,
    });
  });
  await waitFor(() => expect(windowHandler).not.toBeNull());
  expect(toggle).toHaveAttribute("aria-checked", "true");
  expect(toggle).toBeDisabled();
  act(() => {
    for (const w of windows) {
      windowHandler!({ ...w, session_id: 1 });
    }
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  windowHandler = null;
  partialHandler = null;
  liveCaptureHandler = null;
  liveCaptureRevision = 0;
  vi.mocked(ipc.listStreamingSessions).mockResolvedValue([]);
  vi.mocked(ipc.listTaskModels).mockResolvedValue([LLM_MODEL_READY]);
  vi.mocked(ipc.getSettings).mockResolvedValue({
    theme: "system",
    ui_language: "en",
    active_model_diarization: "none",
    export_file_type: "plain_text",
    active_model_llm: "llm-mini",
  });
  vi.mocked(ipc.listStreamingTranslations).mockResolvedValue([]);
  vi.mocked(ipc.setStreamingTranslationEnabled).mockResolvedValue(undefined);
  previewStreamingTranslationMock.mockResolvedValue("Draft preview.");
});

describe("StreamingView — Live Translation header control", () => {
  it("renders the languages icon + switch + target-language select in a middle slot between the title group and the action cluster", async () => {
    const user = userEvent.setup();
    await openSessionWithWindows(user, TWO_PARAGRAPHS);

    const header = document.querySelector(".wp-transcript-header");
    expect(header).not.toBeNull();
    const titleGroup = header!.querySelector(".wp-transcript-title-group");
    const control = header!.querySelector(".wp-translation-control");
    const actions = header!.querySelector(".wp-transcript-actions");
    expect(control).not.toBeNull();
    expect(control!.textContent).not.toContain("Live Translation");
    expect(control!.querySelector("svg")).not.toBeNull();

    const children = Array.from(header!.children);
    expect(children.indexOf(titleGroup as Element)).toBeLessThan(
      children.indexOf(control as Element),
    );
    expect(children.indexOf(control as Element)).toBeLessThan(
      children.indexOf(actions as Element),
    );

    const toggle = await findTranslationSwitch();
    expect(toggle).toHaveAttribute("aria-checked", "false");
    const select = findTargetLanguageSelect();
    expect(select).toHaveValue("ru");
    expect(select).not.toBeDisabled();
  });

  it("the switch and select are keyboard-focusable with accessible names", async () => {
    const user = userEvent.setup();
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();

    toggle.focus();
    expect(toggle).toHaveFocus();

    const select = findTargetLanguageSelect();
    select.focus();
    expect(select).toHaveFocus();

    await user.selectOptions(select, "en");
    expect(ipc.setStreamingTranslationTargetLanguage).toHaveBeenCalledWith(
      1,
      "en",
    );
  });
});

describe("StreamingView — Live Translation split grid", () => {
  it("switching on renders a two-column paired-row grid, one row per paragraph, inside the single existing scroll container", async () => {
    const user = userEvent.setup();
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();

    await user.click(toggle);

    expect(await screen.findByText(SOURCE_A)).toBeInTheDocument();
    expect(screen.getByText(SOURCE_B)).toBeInTheDocument();
    expect(screen.getByText("ORIGINAL · AUTO-DETECTED")).toBeInTheDocument();
    expect(screen.getByText("РУССКИЙ")).toBeInTheDocument();

    const scrollContainers = document.querySelectorAll(
      ".wp-transcript-content",
    );
    expect(scrollContainers).toHaveLength(1);
    expect(
      scrollContainers[0].querySelector(".wp-translation-grid"),
    ).not.toBeNull();
    expect(document.querySelectorAll(".wp-translation-row")).toHaveLength(2);
  });

  it("switching off restores the single-column rendering unchanged, and re-enables the select", async () => {
    const user = userEvent.setup();
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();

    await user.click(toggle);
    expect(await screen.findByText(SOURCE_A)).toBeInTheDocument();

    await user.click(toggle);

    expect(document.querySelector(".wp-translation-grid")).toBeNull();
    expect(document.querySelectorAll(".streaming-paragraph")).toHaveLength(2);
    expect(findTargetLanguageSelect()).not.toBeDisabled();
  });

  it("renders a live partial in the Original column while translation is enabled", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.translateStreamingWindow).mockReturnValue(
      new Promise(() => {}),
    );
    await startRunningSessionWithWindows(
      user,
      makeWindows(1, { startIndex: 0, language: "ru" }),
    );
    await waitFor(() => expect(partialHandler).not.toBeNull());

    act(() => {
      partialHandler!({
        session_id: 1,
        item_id: null,
        text: "Ещё не законченная фраза",
      });
    });

    const partial = document.querySelector(".wp-streaming-partial");
    expect(partial).not.toBeNull();
    const originalColumn = partial!.closest(".wp-translation-col");
    expect(originalColumn).not.toBeNull();
    const row = originalColumn!.closest(".wp-translation-columns");
    const columns = Array.from(
      row!.querySelectorAll(":scope > .wp-translation-col"),
    );
    expect(columns[0]).toBe(originalColumn);
  });
});

describe("StreamingView — provisional Live Translation", () => {
  it("keeps the whole source hypothesis italic until a committed window replaces it", async () => {
    const user = userEvent.setup();
    await startRunningSessionWithWindows(user, []);
    await waitFor(() => expect(partialHandler).not.toBeNull());

    act(() => {
      partialHandler!({
        session_id: 1,
        item_id: null,
        text: "Still provisional",
      });
    });
    await waitFor(() =>
      expect(screen.getByText("Still provisional")).toBeInTheDocument(),
    );
    act(() => {
      partialHandler!({
        session_id: 1,
        item_id: null,
        text: "Still provisional",
      });
    });

    const source = document.querySelector(
      ".wp-translation-row--partial .wp-translation-col:first-child",
    );
    expect(source?.querySelector("em")).toHaveTextContent("Still provisional");
    expect(source?.querySelector(".wp-streaming-partial-stable")).toBeNull();
  });

  it("renders a translated partial as italic preview in the right column", async () => {
    const user = userEvent.setup();
    previewStreamingTranslationMock.mockResolvedValue("Черновой перевод.");
    await startRunningSessionWithWindows(user, []);
    await waitFor(() => expect(partialHandler).not.toBeNull());

    act(() => {
      partialHandler!({
        session_id: 1,
        item_id: null,
        text: "An unfinished live sentence",
      });
    });

    await waitFor(() =>
      expect(previewStreamingTranslationMock).toHaveBeenCalledWith(
        "ru",
        "An unfinished live sentence",
        undefined,
      ),
    );
    const row = document.querySelector(".wp-translation-row--partial");
    expect(row).not.toBeNull();
    const columns = row!.querySelectorAll(":scope > .wp-translation-col");
    const preview = await waitFor(() => {
      const value = Array.from(columns[1].querySelectorAll("em")).find(
        (element) => element.textContent === "Черновой перевод.",
      );
      expect(value).toBeDefined();
      return value!;
    });
    expect(preview.tagName).toBe("EM");
    expect(columns[1]).not.toHaveAttribute("aria-hidden");
  });

  it("coalesces partial updates while one preview is in flight and translates only the latest pending text next", async () => {
    const first = deferred<string>();
    previewStreamingTranslationMock
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce("Latest draft.");
    const user = userEvent.setup();
    await startRunningSessionWithWindows(user, []);
    await waitFor(() => expect(partialHandler).not.toBeNull());

    act(() => {
      partialHandler!({ session_id: 1, item_id: null, text: "First partial" });
    });
    await waitFor(() =>
      expect(previewStreamingTranslationMock).toHaveBeenCalledWith(
        "ru",
        "First partial",
        undefined,
      ),
    );

    act(() => {
      partialHandler!({ session_id: 1, item_id: null, text: "Skipped middle" });
      partialHandler!({ session_id: 1, item_id: null, text: "Latest partial" });
    });
    expect(previewStreamingTranslationMock).toHaveBeenCalledTimes(1);

    await act(async () => first.resolve("Stale draft."));
    await waitFor(() =>
      expect(previewStreamingTranslationMock).toHaveBeenNthCalledWith(
        2,
        "ru",
        "Latest partial",
        undefined,
      ),
    );
    expect(previewStreamingTranslationMock).toHaveBeenCalledTimes(2);
  });

  it("keeps the last usable translated draft visible while a newer draft is translating", async () => {
    const first = deferred<string>();
    const second = deferred<string>();
    previewStreamingTranslationMock
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const user = userEvent.setup();
    await startRunningSessionWithWindows(user, []);
    await waitFor(() => expect(partialHandler).not.toBeNull());

    act(() => {
      partialHandler!({ session_id: 1, item_id: null, text: "First partial" });
    });
    await waitFor(() =>
      expect(previewStreamingTranslationMock).toHaveBeenCalledTimes(1),
    );
    act(() => {
      partialHandler!({
        session_id: 1,
        item_id: null,
        text: "First partial extended",
      });
    });

    await act(async () => first.resolve("Первый черновик."));
    await waitFor(() =>
      expect(previewStreamingTranslationMock).toHaveBeenCalledTimes(2),
    );

    expect(screen.getByText("Первый черновик.")).toBeInTheDocument();
    expect(screen.queryByText("Draft translation…")).not.toBeInTheDocument();
  });

  it("lets a committed translation replace a preview and ignores a late preview result", async () => {
    const preview = deferred<string>();
    previewStreamingTranslationMock.mockReturnValue(preview.promise);
    vi.mocked(ipc.translateStreamingWindow).mockResolvedValue(
      "Final committed translation.",
    );
    const user = userEvent.setup();
    await startRunningSessionWithWindows(user, []);
    await waitFor(() => expect(partialHandler).not.toBeNull());

    act(() => {
      partialHandler!({
        session_id: 1,
        item_id: null,
        text: "Sentence being recognized",
      });
    });
    await waitFor(() =>
      expect(previewStreamingTranslationMock).toHaveBeenCalledOnce(),
    );

    act(() => {
      windowHandler!({
        ...makeWindows(1, { prefix: "Sentence committed" })[0],
        session_id: 1,
      });
    });
    expect(
      await screen.findByText("Final committed translation."),
    ).toBeInTheDocument();

    await act(async () => preview.resolve("Late stale preview."));
    await act(async () => flush());

    expect(screen.queryByText("Late stale preview.")).not.toBeInTheDocument();
    expect(
      screen.getByText("Final committed translation."),
    ).toBeInTheDocument();
  });

  it("does not attach preview A to partial B after A was committed", async () => {
    const previewA = deferred<string>();
    previewStreamingTranslationMock.mockReturnValue(previewA.promise);
    vi.mocked(ipc.translateStreamingWindow).mockResolvedValue(
      "Committed translation A.",
    );
    const user = userEvent.setup();
    await startRunningSessionWithWindows(user, []);
    await waitFor(() => expect(partialHandler).not.toBeNull());

    act(() => {
      partialHandler!({ session_id: 1, item_id: null, text: "Partial A" });
    });
    await waitFor(() =>
      expect(previewStreamingTranslationMock).toHaveBeenCalledOnce(),
    );
    act(() => {
      windowHandler!({
        ...makeWindows(1, { prefix: "Committed A" })[0],
        session_id: 1,
      });
      partialHandler!({ session_id: 1, item_id: null, text: "Partial B" });
    });

    await act(async () => previewA.resolve("Stale preview A."));
    await act(async () => flush());

    expect(screen.queryByText("Stale preview A.")).not.toBeInTheDocument();
  });
});

// WP-103/WP-113: translation is per-window and always processed strictly in
// increasing window_index order through the single-flight queue. A committed
// live window must not wait for a later window or for capture to stop.
describe("StreamingView — Live Translation per-window triggering (WP-103)", () => {
  it("@WP-113-live-translation: translates the first newly committed window while capture remains active", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.translateStreamingWindow).mockResolvedValue(
      "Translated live window.",
    );
    await startRunningSessionWithWindows(
      user,
      makeWindows(1, { startIndex: 0 }),
    );

    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
        1,
        0,
        "ru",
        "Слово0",
        undefined,
      ),
    );
    expect(screen.getByRole("button", { name: "Stop" })).not.toBeDisabled();
    expect(ipc.stopStreamingSession).not.toHaveBeenCalled();
  });

  it("@WP-103-bootstrap: window 0 starts immediately, then window 1 follows with window 0's translation as context", async () => {
    const user = userEvent.setup();
    const w0 = deferred<string>();
    vi.mocked(ipc.translateStreamingWindow).mockImplementation(
      (_session, windowIndex) =>
        windowIndex === 0 ? w0.promise : Promise.resolve("W1 translated."),
    );

    await startRunningSessionWithWindows(
      user,
      makeWindows(2, { startIndex: 0 }),
    );

    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
        1,
        0,
        "ru",
        "Слово0",
        undefined,
      ),
    );
    // Single-flight: window 1 is enqueued alongside window 0 in the same
    // reconcile pass, but not yet dequeued/sent — only one call in flight.
    expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(1);

    w0.resolve("W0 translated.");

    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
        1,
        1,
        "ru",
        "Слово1",
        "W0 translated.",
      ),
    );
  });

  it("@WP-103-rolling-context: a mid-session window translates using the concatenation of its 2 immediately preceding windows' translations as context", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.translateStreamingWindow).mockImplementation(
      (_session, windowIndex) => Promise.resolve(`W${windowIndex} translated.`),
    );

    await startRunningSessionWithWindows(
      user,
      makeWindows(5, { startIndex: 0 }),
    );

    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
        1,
        4,
        "ru",
        "Слово4",
        "W2 translated. W3 translated.",
      ),
    );
  });

  it("@WP-103-failed-predecessor: a failed immediately-preceding window is skipped when assembling context, without blocking translation", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.translateStreamingWindow).mockImplementation(
      (_session, windowIndex) =>
        windowIndex === 3
          ? Promise.reject(new Error("model unavailable"))
          : Promise.resolve(`W${windowIndex} translated.`),
    );

    await startRunningSessionWithWindows(
      user,
      makeWindows(5, { startIndex: 0 }),
    );

    // Window 4's context is window 2's translation alone — window 3 failed
    // and is skipped rather than blocking or being substituted for.
    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
        1,
        4,
        "ru",
        "Слово4",
        "W2 translated.",
      ),
    );
  });

  it("@WP-103-backfill: backfills an already-fully-transcribed stopped session oldest-first, one call in flight at a time, with rolling context", async () => {
    const user = userEvent.setup();
    const w0 = deferred<string>();
    const w1 = deferred<string>();
    vi.mocked(ipc.translateStreamingWindow).mockImplementation(
      (_session, windowIndex) => {
        if (windowIndex === 0) return w0.promise;
        if (windowIndex === 1) return w1.promise;
        return Promise.resolve(`W${windowIndex} translated.`);
      },
    );
    await openSessionWithWindows(user, makeWindows(3, { startIndex: 0 }));

    await user.click(await findTranslationSwitch());

    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(1),
    );
    expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
      1,
      0,
      "ru",
      "Слово0",
      undefined,
    );

    w0.resolve("W0 translated.");
    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(2),
    );
    expect(ipc.translateStreamingWindow).toHaveBeenLastCalledWith(
      1,
      1,
      "ru",
      "Слово1",
      "W0 translated.",
    );

    w1.resolve("W1 translated.");
    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(3),
    );
    expect(ipc.translateStreamingWindow).toHaveBeenLastCalledWith(
      1,
      2,
      "ru",
      "Слово2",
      "W0 translated. W1 translated.",
    );
  });

  it("loads persisted translations first and reuses one — matched by window_index and that window's own current text — without a model call, still feeding it as context to the next window", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingTranslations).mockResolvedValue([
      { window_index: 0, source_text: "Слово0", translated_text: "Cached W0." },
    ]);
    const pending = deferred<string>();
    vi.mocked(ipc.translateStreamingWindow).mockReturnValue(pending.promise);
    await openSessionWithWindows(user, makeWindows(2, { startIndex: 0 }));

    await user.click(await findTranslationSwitch());

    expect(ipc.listStreamingTranslations).toHaveBeenCalledWith(1, "ru");
    expect(ipc.translateStreamingWindow).not.toHaveBeenCalledWith(
      1,
      0,
      "ru",
      "Слово0",
      undefined,
    );
    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
        1,
        1,
        "ru",
        "Слово1",
        "Cached W0.",
      ),
    );
    await expectTranslatedCellText("Слово0 Слово1", "Cached W0. Translating…");
  });

  // state-transition: OFF -> ON -> OFF -> ON within the same session must
  // re-gate the reconcile effect on a fresh persisted-translations fetch
  // each time it turns on, not just the first time — a stale `persistedReady`
  // left over from the earlier activation must not let the reconcile effect
  // run before the second fetch resolves.
  it("reuses a persisted translation on a second activation within the same session, without a stray model call", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingTranslations).mockResolvedValue([
      { window_index: 0, source_text: "Слово0", translated_text: "Cached W0." },
      { window_index: 1, source_text: "Слово1", translated_text: "Cached W1." },
    ]);
    await openSessionWithWindows(user, makeWindows(2, { startIndex: 0 }));
    const toggle = await findTranslationSwitch();

    await user.click(toggle); // ON: persisted fetch resolves, reused.
    await expectTranslatedCellText("Слово0 Слово1", "Cached W0. Cached W1.");
    expect(ipc.translateStreamingWindow).not.toHaveBeenCalled();

    await user.click(toggle); // OFF
    expect(document.querySelector(".wp-translation-grid")).toBeNull();

    await user.click(toggle); // ON again, same session + target language.

    expect(ipc.translateStreamingWindow).not.toHaveBeenCalled();
    await expectTranslatedCellText("Слово0 Слово1", "Cached W0. Cached W1.");
    expect(ipc.translateStreamingWindow).not.toHaveBeenCalled();
  });

  it("re-translates instead of reusing a persisted row whose stored source text no longer matches the window's current text", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingTranslations).mockResolvedValue([
      {
        window_index: 0,
        source_text: "stale text",
        translated_text: "Stale cached.",
      },
    ]);
    vi.mocked(ipc.translateStreamingWindow).mockResolvedValue(
      "Fresh translation.",
    );
    await openSessionWithWindows(user, makeWindows(2, { startIndex: 0 }));

    await user.click(await findTranslationSwitch());

    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
        1,
        0,
        "ru",
        "Слово0",
        undefined,
      ),
    );
    expect(screen.queryByText("Stale cached.")).not.toBeInTheDocument();
  });
});

// WP-103: a window whose own language already matches the target is
// mirrored individually — no model call, own text as both source and
// "translated" text — applied per window rather than only when an entire
// paragraph is single-language.
describe("StreamingView — Live Translation same-language mirroring", () => {
  it("never calls the model for a window whose language is already the target language, and mirrors its own text", async () => {
    const user = userEvent.setup();
    const russianWindows = makeWindows(2, {
      startIndex: 0,
      language: "ru",
      prefix: "Слово",
    });
    await openSessionWithWindows(user, russianWindows);

    await user.click(await findTranslationSwitch());

    const source = paragraphSourceText(russianWindows);
    await waitFor(() => {
      const row = document.querySelector(".wp-translation-row");
      const cols = row?.querySelectorAll(".wp-translation-col");
      const translatedCell = cols?.[1]?.querySelector(".wp-translation-text");
      expect(translatedCell?.textContent).toBe(source);
    });
    expect(ipc.translateStreamingWindow).not.toHaveBeenCalled();
  });

  // WP-103's defining improvement over the old all-or-nothing paragraph
  // check: a paragraph mixing an already-target-language window with a
  // window that still needs translation must render both correctly.
  it("@WP-103-mixed-mirror: a paragraph mixing a mirrored window and a translated window renders both correctly, and only the non-mirrored window gets a model call", async () => {
    const user = userEvent.setup();
    const mirroredWindow = makeWindows(1, {
      startIndex: 0,
      language: "ru",
      prefix: "Слово",
    })[0];
    const translatedWindow = makeWindows(1, {
      startIndex: 1,
      language: "en",
      prefix: "Word",
    })[0];
    vi.mocked(ipc.translateStreamingWindow).mockResolvedValue("Мир.");
    await openSessionWithWindows(user, [mirroredWindow, translatedWindow]);

    await user.click(await findTranslationSwitch());

    await expectTranslatedCellText("Слово0 Word1", "Слово0 Мир.");
    expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(1);
    expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
      1,
      1,
      "ru",
      "Word1",
      "Слово0",
    );
  });
});

describe("StreamingView — Live Translation failure and retry", () => {
  it("shows a paragraph-level retry control on a window's failure, continues the queue with later windows, and raises no dialog or capture interruption", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.translateStreamingWindow).mockImplementation(
      (_session, windowIndex) =>
        windowIndex === 0
          ? Promise.reject(new Error("model unavailable"))
          : Promise.resolve(`W${windowIndex} translated.`),
    );
    await startRunningSessionWithWindows(user, [
      ...TWO_PARAGRAPHS,
      // A 9th window keeps the session "running" with an open trailing
      // paragraph, matching how a live capture actually looks mid-session.
      ...makeWindows(1, { startIndex: 8 }),
    ]);

    const retry = await screen.findByRole("button", {
      name: /Translation failed.*Retry/i,
    });
    expect(retry).toBeInTheDocument();
    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
        1,
        7,
        "ru",
        "Слово7.",
        expect.any(String),
      ),
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    // Capture itself was never interrupted by the translation failure.
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(stop).not.toBeDisabled();
    expect(ipc.stopStreamingSession).not.toHaveBeenCalled();
  });

  // WP-103: retry stays a single paragraph-level affordance that re-enqueues
  // every currently-FAILED window within that paragraph (not every window),
  // leaving already-done/mirrored siblings untouched.
  it("retry re-enqueues only the failed windows within that paragraph, recomputing context from current state, and leaves other windows unaffected", async () => {
    const user = userEvent.setup();
    let attemptsForWindow0 = 0;
    vi.mocked(ipc.translateStreamingWindow).mockImplementation(
      (_session, windowIndex) => {
        if (windowIndex === 0) {
          attemptsForWindow0 += 1;
          return attemptsForWindow0 === 1
            ? Promise.reject(new Error("model unavailable"))
            : Promise.resolve("Recovered W0.");
        }
        return Promise.resolve(`W${windowIndex} translated.`);
      },
    );
    await openSessionWithWindows(user, PARAGRAPH_A);
    await user.click(await findTranslationSwitch());
    const retry = await screen.findByRole("button", {
      name: /Translation failed.*Retry/i,
    });
    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(4),
    );

    await user.click(retry);

    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(5),
    );
    // Retried with the paragraph's other (already-succeeded) windows as
    // available context, not the original undefined.
    expect(ipc.translateStreamingWindow).toHaveBeenLastCalledWith(
      1,
      0,
      "ru",
      "Слово0",
      undefined,
    );
    await expectTranslatedCellText(
      SOURCE_A,
      "Recovered W0. W1 translated. W2 translated. W3 translated.",
    );
  });
});

describe("StreamingView — Live Translation / Prettify mutual exclusion and readiness", () => {
  it("locks the target-language select while the switch is on", async () => {
    const user = userEvent.setup();
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();

    await user.click(toggle);

    const select = findTargetLanguageSelect();
    expect(select).toBeDisabled();
    expect(select.getAttribute("title")).toMatch(/turn off live translation/i);
  });

  it("disables the switch with a stated reason when no LLM model is ready", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listTaskModels).mockResolvedValue([]);
    vi.mocked(ipc.getSettings).mockResolvedValue({
      theme: "system",
      ui_language: "en",
      active_model_diarization: "none",
      export_file_type: "plain_text",
    });
    await openSessionWithWindows(user, TWO_PARAGRAPHS);

    const toggle = await findDisabledTranslationSwitch();
    await waitFor(() => expect(toggle).toBeDisabled());
    expect(toggle.getAttribute("title")).toMatch(/language model/i);
  });

  it("disables the switch with a stated reason while a prettified transcript is active", async () => {
    const user = userEvent.setup();
    await openSessionWithWindows(user, TWO_PARAGRAPHS, {
      prettified_text: "Cleaned transcript.",
    });

    const toggle = await findDisabledTranslationSwitch();
    expect(toggle).toBeDisabled();
    await waitFor(() =>
      expect(toggle.getAttribute("title")).toMatch(/prettify/i),
    );
  });

  it("disables the switch with a stated reason while a Prettify review is pending", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.generateStreamingPrettify).mockResolvedValue(
      "Cleaned draft.",
    );
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    await user.click(
      await screen.findByRole("button", { name: "Prettify transcript" }),
    );
    await screen.findByRole("button", { name: "Accept Prettify" });

    const toggle = await findDisabledTranslationSwitch();
    expect(toggle).toBeDisabled();
    await waitFor(() =>
      expect(toggle.getAttribute("title")).toMatch(/prettify/i),
    );
  });

  it("disables Prettify with a stated reason while Live Translation is on", async () => {
    const user = userEvent.setup();
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();

    await user.click(toggle);

    const prettify = screen.getByRole("button", {
      name: "Prettify transcript",
    });
    expect(prettify).toBeDisabled();
    expect(prettify.getAttribute("title")).toMatch(/live translation/i);
  });
});

describe("StreamingView — Live Translation edge cases", () => {
  // state-transition: ON -> OFF while a call is in flight (mid-flight guard
  // condition) must discard that call's result and not resume the queue.
  it("switching off mid-queue cancels pending work and does not continue the queue once the in-flight call resolves", async () => {
    const user = userEvent.setup();
    const first = deferred<string>();
    vi.mocked(ipc.translateStreamingWindow).mockReturnValue(first.promise);
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();

    await user.click(toggle);
    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(1),
    );

    await user.click(toggle);
    first.resolve("Word batch A (en).");
    await flush();

    expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(1);
  });

  // state-transition: an active-session event (session-switch) while ON
  // and mid-flight must transition the switch back to OFF and discard the
  // in-flight call's result rather than let it land in the new session.
  it("switching sessions clears the queue and turns the switch back off", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([
      SESSION_A,
      { ...SESSION_A, id: 2, title: "Design Review" },
    ]);
    vi.mocked(ipc.openStreamingSession).mockImplementation(async (id) =>
      openedSession({
        id,
        title: id === 1 ? "Standup" : "Design Review",
        windows: id === 1 ? TWO_PARAGRAPHS : [],
      }),
    );
    const first = deferred<string>();
    vi.mocked(ipc.translateStreamingWindow).mockReturnValue(first.promise);
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(1),
    );

    await user.click(await screen.findByText("Design Review"));

    const toggleAfterSwitch = await findTranslationSwitch();
    expect(toggleAfterSwitch).toHaveAttribute("aria-checked", "false");
    first.resolve("Word batch A (en).");
    await flush();
    expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(1);
  });

  it("an old completion dequeues a new session only with that session's target language", async () => {
    const user = userEvent.setup();
    const first = deferred<string>();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([
      SESSION_A,
      { ...SESSION_A, id: 2, title: "Design Review" },
    ]);
    vi.mocked(ipc.openStreamingSession).mockImplementation(async (id) =>
      openedSession({
        id,
        title: id === 1 ? "Standup" : "Design Review",
        windows:
          id === 1
            ? makeWindows(1, { prefix: "Old" })
            : makeWindows(1, { prefix: "New", language: "ru" }),
        translation_enabled: id === 2,
        translation_target_language: id === 2 ? "en" : "ru",
      }),
    );
    vi.mocked(ipc.translateStreamingWindow)
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce("New translation.");
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));
    await user.click(await findTranslationSwitch());
    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
        1,
        0,
        "ru",
        "Old0",
        undefined,
      ),
    );

    await user.click(await screen.findByText("Design Review"));
    await waitFor(() => expect(findTargetLanguageSelect()).toHaveValue("en"));
    first.resolve("Discarded old translation.");

    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenLastCalledWith(
        2,
        0,
        "en",
        "New0",
        undefined,
      ),
    );
  });
});

// WP-101: two related bugs from a live run — (1) pressing Start to resume
// the session that is already open turned Live Translation back off and
// cleared the in-progress queue, since startSession() unconditionally reset
// translation state; (2) the enabled/disabled choice never survived
// reopening a session or an app restart, since translationEnabled was plain
// React state with no backing column. Fixed by only resetting translation
// state when the session identity actually changes (resumeId is null or
// differs from activeId), and by persisting the toggle per session
// (mirroring WP-96's MFU-panel settings-write pattern: best-effort, a
// failure never blocks or reverts the switch).
describe("StreamingView — Live Translation session-lifecycle persistence (WP-101)", () => {
  it("Bug 1 repro: pressing Start/Resume on the already-open session leaves Live Translation on and keeps the in-progress translation", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue(ACTIVE_SESSION_A);
    vi.mocked(ipc.translateStreamingWindow).mockResolvedValue("Translated W0.");
    await openSessionWithWindows(user, makeWindows(2, { startIndex: 0 }));
    const toggle = await findTranslationSwitch();

    await user.click(toggle);
    await expectTranslatedCellText(
      "Слово0 Слово1",
      "Translated W0. Translated W0.",
    );

    const startButton = await screen.findByRole("button", {
      name: /^(start|resume)$/i,
    });
    await user.click(startButton);

    // resumeId === activeId (both 1) — the exact "flip the switch on, then
    // press Start" sequence that used to force the switch back off.
    expect(ipc.startStreamingSession).toHaveBeenCalledWith(1);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    // The queue/map was not cleared: the already-translated windows are
    // still shown, not reset to "Pending…".
    await expectTranslatedCellText(
      "Слово0 Слово1",
      "Translated W0. Translated W0.",
    );
  });

  it("starting a brand-new session preserves a Live Translation choice made before capture", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 9,
      title: "New Streaming Session",
      created_at_ms: 100,
      updated_at_ms: 100,
      status: "active",
      translation_enabled: false,
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    liveCaptureRevision += 1;
    act(() => {
      liveCaptureHandler!({
        phase: "capturing",
        session_id: 9,
        source: "streaming",
        generation: 1,
        revision: liveCaptureRevision,
        error: null,
      });
    });

    expect(toggle).toHaveAttribute("aria-checked", "true");
    await waitFor(() => expect(toggle).toBeDisabled());
  });

  it("opening a session whose Live Translation was left on restores the switch to on, with no user action", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.openStreamingSession).mockResolvedValue(
      openedSession({ translation_enabled: true }),
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));

    const toggle = await findTranslationSwitch();
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  it("restores the stopped session's persisted English translation target instead of reopening it as Russian", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([
      { ...SESSION_A, translation_enabled: true },
    ]);
    vi.mocked(ipc.openStreamingSession).mockResolvedValue({
      ...openedSession({ translation_enabled: true }),
      // The UI seam intentionally describes the missing persistence field:
      // the current DTO ignores it and falls back to the component's `ru`.
      translation_target_language: "en",
    } as StreamingSession & { translation_target_language: "en" | "ru" });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));

    await waitFor(() => expect(findTargetLanguageSelect()).toHaveValue("en"));
    await waitFor(() =>
      expect(ipc.listStreamingTranslations).toHaveBeenCalledWith(1, "en"),
    );
  });

  it("opening a session whose Live Translation was left off restores the switch to off", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
    vi.mocked(ipc.openStreamingSession).mockResolvedValue(
      openedSession({ translation_enabled: false }),
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));

    const toggle = await findTranslationSwitch();
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  it("toggling the switch persists the new value for the open session", async () => {
    const user = userEvent.setup();
    await openSessionWithWindows(user, makeWindows(2, { startIndex: 0 }));
    const toggle = await findTranslationSwitch();

    await user.click(toggle);
    expect(ipc.setStreamingTranslationEnabled).toHaveBeenCalledWith(1, true);

    await user.click(toggle);
    expect(ipc.setStreamingTranslationEnabled).toHaveBeenLastCalledWith(
      1,
      false,
    );
  });

  it("keeps the switch showing the user's chosen state, with no blocking error, when persisting the toggle fails", async () => {
    vi.mocked(ipc.setStreamingTranslationEnabled).mockRejectedValue(
      new Error("disk full"),
    );
    const user = userEvent.setup();
    await openSessionWithWindows(user, makeWindows(2, { startIndex: 0 }));
    const toggle = await findTranslationSwitch();

    await user.click(toggle);

    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("opening a different session reflects that session's own persisted value, not the previously open session's", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([
      SESSION_A,
      { ...SESSION_A, id: 2, title: "Design Review" },
    ]);
    vi.mocked(ipc.openStreamingSession).mockImplementation(async (id) =>
      openedSession({
        id,
        title: id === 1 ? "Standup" : "Design Review",
        translation_enabled: id === 2,
      }),
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));
    expect(await findTranslationSwitch()).toHaveAttribute(
      "aria-checked",
      "false",
    );

    await user.click(await screen.findByText("Design Review"));
    expect(await findTranslationSwitch()).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("creating a brand-new session still starts with Live Translation off (nothing persisted yet)", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.createStreamingSession).mockResolvedValue({
      id: 3,
      title: "New Streaming Session",
      created_at_ms: 100,
      updated_at_ms: 100,
      status: "stopped",
      translation_enabled: false,
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(
      await screen.findByRole("button", { name: "New streaming session" }),
    );

    const toggle = await findTranslationSwitch();
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });
});

// WP-102: a stale `persistedReady` left over from a previously open session
// raced the reconcile effect ahead of the reopened session's own
// persisted-translations fetch, re-sending already-translated windows.
describe("StreamingView — Live Translation persisted-cache reload race (WP-102)", () => {
  it("reopening a session with an already-persisted window reuses it without a new model call, even after visiting another session in between", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingSessions).mockResolvedValue([
      { ...SESSION_A, translation_enabled: true },
      {
        ...SESSION_A,
        id: 2,
        title: "Design Review",
        translation_enabled: false,
      },
    ]);
    vi.mocked(ipc.openStreamingSession).mockImplementation(async (id) =>
      id === 1
        ? openedSession({
            id: 1,
            title: "Standup",
            windows: makeWindows(2, { startIndex: 0 }),
            translation_enabled: true,
          })
        : openedSession({
            id: 2,
            title: "Design Review",
            windows: [],
            translation_enabled: false,
          }),
    );
    vi.mocked(ipc.listStreamingTranslations).mockImplementation(
      async (sessionId) =>
        sessionId === 1
          ? [
              {
                window_index: 0,
                source_text: "Слово0",
                translated_text: "Cached W0.",
              },
              {
                window_index: 1,
                source_text: "Слово1",
                translated_text: "Cached W1.",
              },
            ]
          : [],
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);

    await user.click(await screen.findByText("Standup"));
    await expectTranslatedCellText("Слово0 Слово1", "Cached W0. Cached W1.");
    expect(ipc.translateStreamingWindow).not.toHaveBeenCalled();

    await user.click(await screen.findByText("Design Review"));
    await user.click(await screen.findByText("Standup"));

    // Session A's persisted-translations fetch runs once per open (not on
    // the Design Review visit, since its own translation is off); wait for
    // the reopen's fetch to resolve before asserting nothing was enqueued.
    await waitFor(() =>
      expect(ipc.listStreamingTranslations).toHaveBeenCalledTimes(2),
    );
    await flush();
    expect(ipc.translateStreamingWindow).not.toHaveBeenCalled();
  });
});
