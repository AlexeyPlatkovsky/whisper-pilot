import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
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
// per *paragraph* to one call per *window*: nothing translates until the
// session has at least 2 windows, windows 0 and 1 then fire back to back
// (window 0 with no context, window 1 with window 0's translation), and
// every window after that translates alone with a rolling up-to-2-window
// context — all through the same single-flight queue, strictly in
// increasing window_index order.

type Handler<T> = (payload: T) => void;

let windowHandler: Handler<
  ipc.StreamingWindow & { session_id: number }
> | null = null;

vi.mock("./ipc", () => ({
  listStreamingSessions: vi.fn(async () => []),
  openStreamingSession: vi.fn(),
  renameStreamingSession: vi.fn(),
  deleteStreamingSession: vi.fn(),
  createStreamingSession: vi.fn(),
  startStreamingSession: vi.fn(),
  stopStreamingSession: vi.fn(),
  generateStreamingMfu: vi.fn(),
  generateStreamingPrettify: vi.fn(),
  acceptStreamingPrettify: vi.fn(),
  revertStreamingPrettify: vi.fn(),
  translateStreamingWindow: vi.fn(async () => "Translated."),
  listStreamingTranslations: vi.fn(async () => []),
  setStreamingTranslationEnabled: vi.fn(),
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

/** `count` windows of monotonically increasing index, each short enough that
 * only paragraphs.ts's window-count cap (4) closes a paragraph — never the
 * length/sentence heuristic — so paragraph boundaries (used for on-screen
 * display grouping and the paragraph-level retry affordance) are
 * deterministic regardless of text content. Default language is "en" — the
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

// Two full (4-window, WP-100's lowered cap) English paragraphs, closed
// regardless of running state.
const PARAGRAPH_A = makeWindows(4, { startIndex: 0 });
const PARAGRAPH_B = makeWindows(4, { startIndex: 4 });
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
  await user.click(await screen.findByRole("button", { name: "Start" }));
  await waitFor(() => expect(windowHandler).not.toBeNull());
  const toggle = await findTranslationSwitch();
  await user.click(toggle);
  for (const w of windows) {
    windowHandler!({ ...w, session_id: 1 });
  }
}

beforeEach(() => {
  vi.clearAllMocks();
  windowHandler = null;
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
});

describe("StreamingView — Live Translation header control", () => {
  it("renders label + switch + target-language select in a middle slot between the title group and the action cluster", async () => {
    const user = userEvent.setup();
    await openSessionWithWindows(user, TWO_PARAGRAPHS);

    const header = document.querySelector(".wp-transcript-header");
    expect(header).not.toBeNull();
    const titleGroup = header!.querySelector(".wp-transcript-title-group");
    const control = header!.querySelector(".wp-translation-control");
    const actions = header!.querySelector(".wp-transcript-actions");
    expect(control).not.toBeNull();
    expect(control!.textContent).toContain("Live Translation");

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
});

// WP-103: translation is per-window, gated on the session having at least 2
// windows, and always processed strictly in increasing window_index order
// through the single-flight queue.
describe("StreamingView — Live Translation per-window triggering (WP-103)", () => {
  it("translates nothing at all while the session has fewer than 2 windows", async () => {
    const user = userEvent.setup();
    await startRunningSessionWithWindows(
      user,
      makeWindows(1, { startIndex: 0 }),
    );

    await flush();

    expect(ipc.translateStreamingWindow).not.toHaveBeenCalled();
  });

  it("@WP-103-bootstrap: once the 2nd window exists, window 0 and window 1 both translate back to back — window 0 with no context, then window 1 with window 0's translation as context", async () => {
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
        "Слово7",
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

  it("starting a brand-new session (resumeId null) still resets Live Translation to off", async () => {
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

    await user.click(await screen.findByRole("button", { name: "Start" }));

    const toggle = await findTranslationSwitch();
    expect(toggle).toHaveAttribute("aria-checked", "false");
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
