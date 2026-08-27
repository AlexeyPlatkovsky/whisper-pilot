import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StreamingView } from "./StreamingView";
import * as ipc from "./ipc";
import type {
  StreamingSession,
  StreamingSessionSummary,
  StreamingWindow,
  TaskModel,
} from "./ipc";

// WP-93: the Live Translation header control (label + switch + locked
// target-language select) and the two-column paired-row transcript grid,
// mirroring the inline vi.mock idiom of StreamingView.test.tsx /
// StreamingView.mfuToggle.test.tsx.

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
  translateStreamingParagraph: vi.fn(async () => "Translated."),
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
 * length/sentence heuristic — so paragraph boundaries are deterministic
 * regardless of text content. Default language is "en" — the mirror image
 * of the "ru" target-language default, so paragraphs built with no override
 * exercise real translation instead of the same-language mirror path. */
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

describe("StreamingView — Live Translation queue", () => {
  it("backfills existing paragraphs oldest-first with at most one translate call in flight, rendering each result in its own row", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 1,
      title: "Standup",
      created_at_ms: 100,
      updated_at_ms: 100,
      status: "active",
      translation_enabled: false,
    });
    const first = deferred<string>();
    const second = deferred<string>();
    vi.mocked(ipc.translateStreamingParagraph).mockImplementation(
      (_session, paragraphKey) =>
        paragraphKey === 0 ? first.promise : second.promise,
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());
    for (const w of [
      ...PARAGRAPH_A,
      ...PARAGRAPH_B,
      ...makeWindows(1, { startIndex: 8 }),
    ]) {
      windowHandler!({ ...w, session_id: 1 });
    }
    await screen.findByText(new RegExp(PARAGRAPH_A[0].text));

    const toggle = await findTranslationSwitch();
    await user.click(toggle);

    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledTimes(1),
    );
    // Both paragraphs are enqueued in the same reconcile pass, so A hasn't
    // resolved yet when B is enqueued — no prior-paragraph context either
    // call.
    expect(ipc.translateStreamingParagraph).toHaveBeenCalledWith(
      1,
      0,
      "ru",
      SOURCE_A,
      undefined,
    );

    const rowA = screen
      .getByText(SOURCE_A)
      .closest(".wp-translation-row") as HTMLElement;
    expect(within(rowA).getByText("Translating…")).toBeInTheDocument();
    const rowB = screen
      .getByText(SOURCE_B)
      .closest(".wp-translation-row") as HTMLElement;
    expect(within(rowB).getByText("Pending…")).toBeInTheDocument();

    first.resolve("Word batch A (en).");
    expect(
      await within(rowA).findByText("Word batch A (en)."),
    ).toBeInTheDocument();

    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledTimes(2),
    );
    expect(ipc.translateStreamingParagraph).toHaveBeenLastCalledWith(
      1,
      4,
      "ru",
      SOURCE_B,
      undefined,
    );

    second.resolve("Word batch B (en).");
    expect(
      await within(rowB).findByText("Word batch B (en)."),
    ).toBeInTheDocument();

    // The trailing, not-yet-closed paragraph (window 8) is excluded from
    // backfill while the session is still running.
    expect(ipc.translateStreamingParagraph).toHaveBeenCalledTimes(2);
  });

  it("loads persisted translations first and reuses a matching one without a model call", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingTranslations).mockResolvedValue([
      { paragraph_key: 0, source_text: SOURCE_A, translated_text: "Cached A." },
    ]);
    const pending = deferred<string>();
    vi.mocked(ipc.translateStreamingParagraph).mockReturnValue(pending.promise);
    await openSessionWithWindows(user, TWO_PARAGRAPHS);

    await user.click(await findTranslationSwitch());

    expect(await screen.findByText("Cached A.")).toBeInTheDocument();
    expect(ipc.listStreamingTranslations).toHaveBeenCalledWith(1, "ru");
    expect(ipc.translateStreamingParagraph).not.toHaveBeenCalledWith(
      1,
      0,
      "ru",
      SOURCE_A,
    );
    // Paragraph B has no persisted row, so it still gets a live call — and
    // (WP-100) since the persisted-reused A entry is already "done" by the
    // time B is reconciled, B's prompt gets A's translation as context.
    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledWith(
        1,
        4,
        "ru",
        SOURCE_B,
        "Cached A.",
      ),
    );
  });

  // WP-100 scenario 1 + DoD: a paragraph that closes purely on the
  // window-count cap, with no next paragraph having started forming, must
  // be enqueued immediately while the session is still running — not held
  // back for a sibling paragraph to appear.
  it("enqueues the last paragraph as soon as it closes via the window-count cap alone, with no next paragraph present", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 1,
      title: "Standup",
      created_at_ms: 100,
      updated_at_ms: 100,
      status: "active",
      translation_enabled: false,
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());

    const toggle = await findTranslationSwitch();
    await user.click(toggle);

    // Exactly MAX_WINDOWS_PER_PARAGRAPH (4) windows, none ending a sentence
    // — only the cap closes this paragraph, and it is the only (last)
    // paragraph in the array.
    for (const w of PARAGRAPH_A) {
      windowHandler!({ ...w, session_id: 1 });
    }

    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledWith(
        1,
        0,
        "ru",
        SOURCE_A,
        undefined,
      ),
    );
  });

  // state-transition: OFF -> ON -> OFF -> ON within the same session must
  // re-gate the reconcile effect on a fresh persisted-translations fetch
  // each time it turns on, not just the first time — a stale `persistedReady`
  // left over from the earlier activation must not let the reconcile effect
  // run before the second fetch resolves.
  it("reuses a persisted translation on a second activation within the same session, without a stray model call", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingTranslations).mockResolvedValue([
      { paragraph_key: 0, source_text: SOURCE_A, translated_text: "Cached A." },
    ]);
    await openSessionWithWindows(user, PARAGRAPH_A);
    const toggle = await findTranslationSwitch();

    await user.click(toggle); // ON: persisted fetch resolves, reused.
    expect(await screen.findByText("Cached A.")).toBeInTheDocument();
    expect(ipc.translateStreamingParagraph).not.toHaveBeenCalled();

    await user.click(toggle); // OFF
    expect(document.querySelector(".wp-translation-grid")).toBeNull();

    await user.click(toggle); // ON again, same session + target language.

    // The persisted row must be reused again — no live call for it, even
    // though the reconcile effect's own commit runs before the second
    // fetch's promise settles.
    expect(ipc.translateStreamingParagraph).not.toHaveBeenCalled();
    expect(await screen.findByText("Cached A.")).toBeInTheDocument();
    expect(ipc.translateStreamingParagraph).not.toHaveBeenCalled();
  });

  it("re-translates instead of reusing a persisted row whose stored source text no longer matches the current paragraph", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.listStreamingTranslations).mockResolvedValue([
      {
        paragraph_key: 0,
        source_text: "stale text",
        translated_text: "Stale cached.",
      },
    ]);
    vi.mocked(ipc.translateStreamingParagraph).mockResolvedValue(
      "Fresh translation.",
    );
    await openSessionWithWindows(user, TWO_PARAGRAPHS);

    await user.click(await findTranslationSwitch());

    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledWith(
        1,
        0,
        "ru",
        SOURCE_A,
        undefined,
      ),
    );
    expect(screen.queryByText("Stale cached.")).not.toBeInTheDocument();
  });
});

describe("StreamingView — Live Translation same-language skip", () => {
  it("never calls the model for a paragraph whose windows are all already the target language, and mirrors the original text", async () => {
    const user = userEvent.setup();
    const russianParagraph = makeWindows(4, {
      startIndex: 0,
      language: "ru",
      prefix: "Слово",
    });
    await openSessionWithWindows(user, russianParagraph);

    await user.click(await findTranslationSwitch());

    const source = paragraphSourceText(russianParagraph);
    const mirrored = await screen.findByText(source, {
      selector: ".wp-translation-text--mirrored",
    });
    expect(mirrored).toBeInTheDocument();
    expect(ipc.translateStreamingParagraph).not.toHaveBeenCalled();
  });
});

// WP-100 scenarios 2 & 3: the immediately preceding paragraph's translation
// is threaded into the next model call as reference-only context, but only
// when that entry is genuinely target-language text (status "done" or
// "mirrored"). A "pending"/"translating"/"failed" previous entry, or no
// entry at all (first paragraph), contributes no context.
describe("StreamingView — Live Translation prior-paragraph context", () => {
  it("passes the preceding paragraph's translation as context once it is done", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 1,
      title: "Standup",
      created_at_ms: 100,
      updated_at_ms: 100,
      status: "active",
      translation_enabled: false,
    });
    vi.mocked(ipc.translateStreamingParagraph).mockImplementation(
      (_session, paragraphKey) =>
        paragraphKey === 0
          ? Promise.resolve("Translated A.")
          : Promise.resolve("Translated B."),
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());
    const toggle = await findTranslationSwitch();
    await user.click(toggle);

    for (const w of PARAGRAPH_A) windowHandler!({ ...w, session_id: 1 });
    await screen.findByText("Translated A.");

    // Paragraph B closes only after A is already "done".
    for (const w of PARAGRAPH_B) windowHandler!({ ...w, session_id: 1 });

    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledWith(
        1,
        4,
        "ru",
        SOURCE_B,
        "Translated A.",
      ),
    );
  });

  it("passes the preceding mirrored paragraph's own text as context", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 1,
      title: "Standup",
      created_at_ms: 100,
      updated_at_ms: 100,
      status: "active",
      translation_enabled: false,
    });
    vi.mocked(ipc.translateStreamingParagraph).mockResolvedValue(
      "Translated B.",
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());
    const toggle = await findTranslationSwitch();
    await user.click(toggle);

    const mirroredParagraph = makeWindows(4, {
      startIndex: 0,
      language: "ru",
      prefix: "Слово",
    });
    for (const w of mirroredParagraph) windowHandler!({ ...w, session_id: 1 });
    const mirroredSource = paragraphSourceText(mirroredParagraph);
    await screen.findByText(mirroredSource, {
      selector: ".wp-translation-text--mirrored",
    });

    // Paragraph B (English, startIndex 4 — not the target language) closes
    // after the mirrored paragraph is already recorded.
    for (const w of PARAGRAPH_B) windowHandler!({ ...w, session_id: 1 });

    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledWith(
        1,
        4,
        "ru",
        SOURCE_B,
        mirroredSource,
      ),
    );
  });

  it("passes no context while the preceding paragraph is still translating", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 1,
      title: "Standup",
      created_at_ms: 100,
      updated_at_ms: 100,
      status: "active",
      translation_enabled: false,
    });
    const first = deferred<string>();
    vi.mocked(ipc.translateStreamingParagraph).mockImplementation(
      (_session, paragraphKey) =>
        paragraphKey === 0 ? first.promise : Promise.resolve("Translated B."),
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());
    const toggle = await findTranslationSwitch();
    await user.click(toggle);

    for (const w of PARAGRAPH_A) windowHandler!({ ...w, session_id: 1 });
    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledTimes(1),
    );

    // Paragraph B closes while A's call is still in flight (single-flight
    // means B is only queued, not yet sent). Flushed before A resolves so
    // B's enqueue observes A as still "translating", not "done".
    for (const w of PARAGRAPH_B) windowHandler!({ ...w, session_id: 1 });
    await flush();
    first.resolve("Translated A.");

    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledWith(
        1,
        4,
        "ru",
        SOURCE_B,
        undefined,
      ),
    );
  });

  it("passes no context when the preceding paragraph failed", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 1,
      title: "Standup",
      created_at_ms: 100,
      updated_at_ms: 100,
      status: "active",
      translation_enabled: false,
    });
    vi.mocked(ipc.translateStreamingParagraph).mockImplementation(
      (_session, paragraphKey) =>
        paragraphKey === 0
          ? Promise.reject(new Error("model unavailable"))
          : Promise.resolve("Translated B."),
    );
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());
    const toggle = await findTranslationSwitch();
    await user.click(toggle);

    for (const w of PARAGRAPH_A) windowHandler!({ ...w, session_id: 1 });
    await screen.findByRole("button", { name: /Translation failed.*Retry/i });

    // Paragraph B closes only after A has already settled into "failed".
    for (const w of PARAGRAPH_B) windowHandler!({ ...w, session_id: 1 });

    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledWith(
        1,
        4,
        "ru",
        SOURCE_B,
        undefined,
      ),
    );
  });

  it("passes no context for the first paragraph of a session", async () => {
    const user = userEvent.setup();
    await openSessionWithWindows(user, PARAGRAPH_A);

    await user.click(await findTranslationSwitch());

    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledWith(
        1,
        0,
        "ru",
        SOURCE_A,
        undefined,
      ),
    );
  });
});

describe("StreamingView — Live Translation failure and retry", () => {
  it("shows a retry control on failure, continues the queue with the next paragraph, and raises no dialog or capture interruption", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.translateStreamingParagraph).mockImplementation(
      (_session, paragraphKey) =>
        paragraphKey === 0
          ? Promise.reject(new Error("model unavailable"))
          : Promise.resolve("Word batch B (en)."),
    );
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 1,
      title: "Standup",
      created_at_ms: 100,
      updated_at_ms: 100,
      status: "active",
      translation_enabled: false,
    });
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() => expect(windowHandler).not.toBeNull());
    for (const w of TWO_PARAGRAPHS) {
      windowHandler!({ ...w, session_id: 1 });
    }
    // A 9th window keeps the session "running" with an open trailing
    // paragraph, matching how a live capture actually looks mid-session.
    windowHandler!({
      ...makeWindows(1, { startIndex: 8 })[0],
      session_id: 1,
    });

    await user.click(await findTranslationSwitch());

    const retry = await screen.findByRole("button", {
      name: /Translation failed.*Retry/i,
    });
    expect(retry).toBeInTheDocument();
    expect(await screen.findByText("Word batch B (en).")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    // Capture itself was never interrupted by the translation failure.
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(stop).not.toBeDisabled();
    expect(ipc.stopStreamingSession).not.toHaveBeenCalled();
  });

  it("retry re-runs only the failed paragraph and leaves other rows unaffected", async () => {
    const user = userEvent.setup();
    let attemptsForA = 0;
    vi.mocked(ipc.translateStreamingParagraph).mockImplementation(
      (_session, paragraphKey) => {
        if (paragraphKey === 0) {
          attemptsForA += 1;
          return attemptsForA === 1
            ? Promise.reject(new Error("model unavailable"))
            : Promise.resolve("Recovered A (en).");
        }
        return Promise.resolve("Word batch B (en).");
      },
    );
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    await user.click(await findTranslationSwitch());
    const retry = await screen.findByRole("button", {
      name: /Translation failed.*Retry/i,
    });
    expect(await screen.findByText("Word batch B (en).")).toBeInTheDocument();

    await user.click(retry);

    expect(await screen.findByText("Recovered A (en).")).toBeInTheDocument();
    expect(ipc.translateStreamingParagraph).toHaveBeenCalledTimes(3);
    expect(screen.getByText("Word batch B (en).")).toBeInTheDocument();
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
    vi.mocked(ipc.translateStreamingParagraph).mockReturnValue(first.promise);
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();

    await user.click(toggle);
    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledTimes(1),
    );

    await user.click(toggle);
    first.resolve("Word batch A (en).");
    await flush();

    expect(ipc.translateStreamingParagraph).toHaveBeenCalledTimes(1);
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
    vi.mocked(ipc.translateStreamingParagraph).mockReturnValue(first.promise);
    render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
    await user.click(await screen.findByText("Standup"));
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledTimes(1),
    );

    await user.click(await screen.findByText("Design Review"));

    const toggleAfterSwitch = await findTranslationSwitch();
    expect(toggleAfterSwitch).toHaveAttribute("aria-checked", "false");
    first.resolve("Word batch A (en).");
    await flush();
    expect(ipc.translateStreamingParagraph).toHaveBeenCalledTimes(1);
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
    vi.mocked(ipc.startStreamingSession).mockResolvedValue({
      id: 1,
      title: "Standup",
      created_at_ms: 100,
      updated_at_ms: 100,
      status: "active",
      translation_enabled: false,
    });
    vi.mocked(ipc.translateStreamingParagraph).mockResolvedValue(
      "Translated A.",
    );
    await openSessionWithWindows(user, PARAGRAPH_A);
    const toggle = await findTranslationSwitch();

    await user.click(toggle);
    expect(await screen.findByText("Translated A.")).toBeInTheDocument();

    const startButton = await screen.findByRole("button", {
      name: /^(start|resume)$/i,
    });
    await user.click(startButton);

    // resumeId === activeId (both 1) — the exact "flip the switch on, then
    // press Start" sequence that used to force the switch back off.
    expect(ipc.startStreamingSession).toHaveBeenCalledWith(1);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    // The queue/map was not cleared: the already-translated paragraph is
    // still shown, not reset to "Pending…".
    expect(screen.getByText("Translated A.")).toBeInTheDocument();
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
    await openSessionWithWindows(user, PARAGRAPH_A);
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
    await openSessionWithWindows(user, PARAGRAPH_A);
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
