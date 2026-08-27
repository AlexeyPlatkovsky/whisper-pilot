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

// WP-94/WP-103: Streaming Copy/Export wiring for the paired
// original+translation renderer (src/export.ts's renderStreamingPaired /
// hasStreamingTranslations), mirroring the inline vi.mock idiom of
// StreamingView.translation.test.tsx and StreamingView.test.tsx's clipboard-
// spy setup. WP-103 moved translation from one call per paragraph to one
// call per *window* — a "paragraph" below is still 4 windows (the
// window-count cap from paragraphs.ts), but each of those 4 windows now
// gets its own translateStreamingWindow call and its own entry.

let writeTextMock: ReturnType<typeof vi.spyOn>;

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
  onStreamingWindow: vi.fn(async () => () => {}),
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
 * only paragraphs.ts's window-count cap (4) closes a paragraph, so paragraph
 * boundaries are deterministic regardless of text content — mirrors
 * StreamingView.translation.test.tsx's helper. Default language is "en" —
 * the mirror image of the "ru" target-language default, so paragraphs built
 * with no override exercise real translation instead of the same-language
 * mirror path. */
function makeWindows(
  count: number,
  opts: { startIndex?: number; language?: string; prefix?: string } = {},
): StreamingWindow[] {
  const { startIndex = 0, language = "en", prefix = "Слово" } = opts;
  return Array.from({ length: count }, (_, i) => {
    const index = startIndex + i;
    return {
      window_index: index,
      start_ms: index * 1000,
      end_ms: index * 1000 + 900,
      text: `${prefix}${index}`,
      language,
      outcome_ok: true,
    };
  });
}

function paragraphSourceText(windows: StreamingWindow[]): string {
  return windows.map((w) => w.text).join(" ");
}

/** The joined translated text a paragraph's cell shows once every one of
 * its windows has resolved via `translatedFor` — mirrors
 * streamingText.ts's `paragraphTranslatedText` join (space-joined). */
function joinedTranslatedText(
  windows: StreamingWindow[],
  translatedFor: (windowIndex: number) => string,
): string {
  return windows.map((w) => translatedFor(w.window_index)).join(" ");
}

const PARAGRAPH_A = makeWindows(4, { startIndex: 0 });
const PARAGRAPH_B = makeWindows(4, { startIndex: 4 });
const SOURCE_A = paragraphSourceText(PARAGRAPH_A);
const SOURCE_B = paragraphSourceText(PARAGRAPH_B);
const TWO_PARAGRAPHS = [...PARAGRAPH_A, ...PARAGRAPH_B];

const PLACEHOLDER = "[Not translated]";
const FAILED_PLACEHOLDER = "[Translation failed]";

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
) {
  vi.mocked(ipc.listStreamingSessions).mockResolvedValue([SESSION_A]);
  vi.mocked(ipc.openStreamingSession).mockResolvedValue(
    openedSession({ windows }),
  );
  render(<StreamingView onClose={vi.fn()} onOpenSettings={vi.fn()} />);
  await user.click(await screen.findByText("Standup"));
  await screen.findByRole("switch", { name: "Live Translation" });
}

// Waits past the async LLM-readiness fetch so a click on the switch doesn't
// race its initial (readiness-pending) disabled state.
async function findTranslationSwitch() {
  const toggle = await screen.findByRole("switch", {
    name: "Live Translation",
  });
  await waitFor(() => expect(toggle).not.toBeDisabled());
  return toggle;
}

// Waits for the post-click "Copied" confirmation so the assertion below
// doesn't race CopyButton's async clipboard write (its onClick handler is
// fire-and-forget, not awaited by userEvent.click).
async function clickCopy(user: ReturnType<typeof userEvent.setup>) {
  await user.click(
    await screen.findByRole("button", { name: "Copy transcript" }),
  );
  await screen.findByRole("button", { name: "Copied" });
}

async function clickExport(user: ReturnType<typeof userEvent.setup>) {
  await user.click(
    await screen.findByRole("button", { name: "Export as Markdown" }),
  );
}

beforeEach(() => {
  vi.clearAllMocks();
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
});

// `userEvent.setup()` itself installs jsdom's clipboard stub the first time
// it runs in this file (getter-based; see @testing-library/user-event's
// Clipboard.attachClipboardStubToView) — spying on navigator.clipboard.
// writeText *before* that first call gets silently discarded when it swaps
// the property out from under the spy. Spying strictly *after* setup() (and
// on every test, since the userEvent-owned stub instance itself is replaced
// between tests too) is what actually intercepts the component's writes.
function setupUser() {
  const user = userEvent.setup();
  if (!navigator.clipboard) {
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: async () => {} },
      configurable: true,
    });
  }
  writeTextMock = vi
    .spyOn(navigator.clipboard, "writeText")
    .mockResolvedValue(undefined);
  return user;
}

describe("StreamingView — paired Copy/Export when Live Translation is off (WP-94)", () => {
  it("@WP-94-off-byte-identical: Copy and Export stay byte-identical to today's single-column output", async () => {
    const user = setupUser();
    await openSessionWithWindows(user, TWO_PARAGRAPHS);

    await clickCopy(user);
    expect(writeTextMock).toHaveBeenCalledWith(`${SOURCE_A} ${SOURCE_B}`);
    expect(writeTextMock.mock.calls[0][0]).not.toContain("Original:");

    await clickExport(user);
    expect(ipc.saveTextDialog).toHaveBeenCalledWith(
      `# Standup\n\n${SOURCE_A} ${SOURCE_B}\n`,
      "Standup.md",
    );
  });

  it("stays byte-identical even after Live Translation was toggled on and back off in the same session", async () => {
    const user = setupUser();
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();

    await user.click(toggle); // ON
    await screen.findByText(SOURCE_A);
    await user.click(toggle); // OFF — clears the translations map.

    await clickCopy(user);
    expect(writeTextMock).toHaveBeenCalledWith(`${SOURCE_A} ${SOURCE_B}`);
  });
});

describe("StreamingView — paired Copy/Export when Live Translation is on (WP-94/WP-103)", () => {
  it("@WP-94-happy-paired-export: Copy places one Original + target-language block per paragraph, each window's own translation joined in screen order", async () => {
    const user = setupUser();
    vi.mocked(ipc.translateStreamingWindow).mockImplementation(
      (_session, windowIndex) => Promise.resolve(`W${windowIndex} (en).`),
    );
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);

    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledTimes(8),
    );
    const translatedA = joinedTranslatedText(PARAGRAPH_A, (i) => `W${i} (en).`);
    const translatedB = joinedTranslatedText(PARAGRAPH_B, (i) => `W${i} (en).`);
    await expectTranslatedCellText(SOURCE_B, translatedB);

    await clickCopy(user);

    expect(writeTextMock).toHaveBeenCalledWith(
      [
        "Original:",
        SOURCE_A,
        "",
        "Русский:",
        translatedA,
        "",
        "Original:",
        SOURCE_B,
        "",
        "Русский:",
        translatedB,
      ].join("\n"),
    );
  });

  it("Export writes the same paired body wrapped in the session's Markdown title heading", async () => {
    const user = setupUser();
    vi.mocked(ipc.translateStreamingWindow).mockImplementation(
      (_session, windowIndex) => Promise.resolve(`W${windowIndex} (en).`),
    );
    await openSessionWithWindows(user, PARAGRAPH_A);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    const translatedA = joinedTranslatedText(PARAGRAPH_A, (i) => `W${i} (en).`);
    await expectTranslatedCellText(SOURCE_A, translatedA);

    await clickExport(user);

    expect(ipc.saveTextDialog).toHaveBeenCalledWith(
      `# Standup\n\nOriginal:\n${SOURCE_A}\n\nРусский:\n${translatedA}\n`,
      "Standup.md",
    );
  });

  it("Copy and Export both render from the same paired text — the two IPC calls carry the identical body", async () => {
    const user = setupUser();
    vi.mocked(ipc.translateStreamingWindow).mockImplementation(
      (_session, windowIndex) => Promise.resolve(`W${windowIndex} (en).`),
    );
    await openSessionWithWindows(user, PARAGRAPH_A);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    const translatedA = joinedTranslatedText(PARAGRAPH_A, (i) => `W${i} (en).`);
    await expectTranslatedCellText(SOURCE_A, translatedA);

    await clickCopy(user);
    await clickExport(user);

    const copied = writeTextMock.mock.calls[0][0] as string;
    const exported = vi.mocked(ipc.saveTextDialog).mock.calls[0][0];
    expect(exported).toBe(`# Standup\n\n${copied}\n`);
  });

  // WP-103: the defining new behavior — a paragraph whose earlier windows
  // already translated and whose trailing window is still in flight shows
  // real text for the finished windows and a placeholder only for that
  // tail, not a blank/all-placeholder cell.
  it("@WP-103-partial-tail: a paragraph with a still-in-flight trailing window exports real text for its finished windows and a placeholder only for the tail", async () => {
    const user = setupUser();
    const windowThree = deferred<string>();
    vi.mocked(ipc.translateStreamingWindow).mockImplementation(
      (_session, windowIndex) => {
        if (windowIndex < 3) return Promise.resolve(`W${windowIndex} (en).`);
        if (windowIndex === 3) return windowThree.promise;
        // Paragraph B's windows never get dequeued: the single-flight queue
        // is blocked on window 3 for the whole test.
        return new Promise<string>(() => {});
      },
    );
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
        1,
        3,
        "ru",
        "Слово3",
        expect.any(String),
      ),
    );
    // Paragraph A's first 3 windows are already visible on screen while
    // window 3 shows its own translating spinner (and all of paragraph B is
    // still pending, blocked behind it in the single-flight queue) — the
    // on-screen cell uses "Translating…"/"Pending…" wording, distinct from
    // the plain-text placeholder Copy/Export use below.
    await expectTranslatedCellText(
      SOURCE_A,
      "W0 (en). W1 (en). W2 (en). Translating…",
    );

    await clickCopy(user);

    expect(writeTextMock).toHaveBeenCalledWith(
      [
        "Original:",
        SOURCE_A,
        "",
        "Русский:",
        "W0 (en). W1 (en). W2 (en). " + PLACEHOLDER,
        "",
        "Original:",
        SOURCE_B,
        "",
        "Русский:",
        [PLACEHOLDER, PLACEHOLDER, PLACEHOLDER, PLACEHOLDER].join(" "),
      ].join("\n"),
    );

    windowThree.resolve("W3 (en).");
  });

  it("@WP-94-placeholder-failed: a window whose translation failed is exported with a distinct failed-translation placeholder, not the not-yet-translated wording", async () => {
    const user = setupUser();
    vi.mocked(ipc.translateStreamingWindow).mockImplementation(
      (_session, windowIndex) =>
        windowIndex === 0
          ? Promise.reject(new Error("model unavailable"))
          : Promise.resolve(`W${windowIndex} (en).`),
    );
    await openSessionWithWindows(user, PARAGRAPH_A);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    // On screen, the failed window renders its own inline wording (no
    // brackets) plus the paragraph-level retry affordance.
    await expectTranslatedCellText(
      SOURCE_A,
      "Translation failed W1 (en). W2 (en). W3 (en).",
    );
    await screen.findByRole("button", { name: /Translation failed.*Retry/i });

    await clickCopy(user);

    // Copy/Export use the distinct bracketed placeholder for a failed
    // window instead of the not-yet-translated wording.
    const exportedText = [
      FAILED_PLACEHOLDER,
      "W1 (en).",
      "W2 (en).",
      "W3 (en).",
    ].join(" ");
    expect(writeTextMock).toHaveBeenCalledWith(
      ["Original:", SOURCE_A, "", "Русский:", exportedText].join("\n"),
    );
  });

  // A mirrored paragraph is not a failure — its windows are already entirely
  // in the target language, so no model call was made by design and the
  // on-screen right cell shows the paragraph's own text. Copy/Export
  // reproduce what the screen shows, so that text must survive verbatim.
  it("@WP-94-mirrored: a same-language mirrored paragraph is exported with its own text verbatim, matching the on-screen muted cell", async () => {
    const user = setupUser();
    const russianParagraph = makeWindows(4, {
      startIndex: 0,
      language: "ru",
      prefix: "Слово",
    });
    const source = paragraphSourceText(russianParagraph);
    await openSessionWithWindows(user, russianParagraph);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    // Both the original and translated columns show `source` verbatim (the
    // point of mirroring) — disambiguate by reading the translated column
    // specifically rather than screen.findByText, which would otherwise
    // match two elements.
    await waitFor(() => {
      const row = document.querySelector(".wp-translation-row");
      const cols = row?.querySelectorAll(".wp-translation-col");
      const translatedCell = cols?.[1]?.querySelector(".wp-translation-text");
      expect(translatedCell?.textContent).toBe(source);
    });

    await clickCopy(user);

    expect(writeTextMock).toHaveBeenCalledWith(
      ["Original:", source, "", "Русский:", source].join("\n"),
    );
    expect(ipc.translateStreamingWindow).not.toHaveBeenCalled();
  });

  it("@WP-94-stale: a persisted translation whose source text no longer matches the current window exports the original with the not-translated placeholder", async () => {
    const user = setupUser();
    vi.mocked(ipc.listStreamingTranslations).mockResolvedValue([
      {
        window_index: 0,
        source_text: "stale text that no longer matches",
        translated_text: "Stale cached translation.",
      },
    ]);
    const pending = deferred<string>();
    vi.mocked(ipc.translateStreamingWindow).mockReturnValue(pending.promise);
    await openSessionWithWindows(user, PARAGRAPH_A);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    await waitFor(() =>
      expect(ipc.translateStreamingWindow).toHaveBeenCalledWith(
        1,
        0,
        "ru",
        "Слово0",
        undefined,
      ),
    );

    await clickCopy(user);

    const copied = writeTextMock.mock.calls[0][0] as string;
    expect(copied).toContain(PLACEHOLDER);
    expect(copied).not.toContain("Stale cached translation.");
  });

  it("@WP-94-language-scope: exports only for the currently selected target language, ignoring rows stored under a different one", async () => {
    const user = setupUser();
    // The backend only ever returns rows for the language it's asked about;
    // simulate a session with a persisted "en" translation but no "ru" one.
    vi.mocked(ipc.listStreamingTranslations).mockImplementation(
      async (_sessionId, lang) =>
        lang === "en"
          ? [
              {
                window_index: 0,
                source_text: "Слово0",
                translated_text: "Cached EN translation.",
              },
            ]
          : [],
    );
    const pending = deferred<string>();
    vi.mocked(ipc.translateStreamingWindow).mockReturnValue(pending.promise);
    await openSessionWithWindows(user, PARAGRAPH_A);
    const toggle = await findTranslationSwitch(); // target language defaults to "ru"
    await user.click(toggle);
    await waitFor(() =>
      expect(ipc.listStreamingTranslations).toHaveBeenCalledWith(1, "ru"),
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

    await clickCopy(user);

    const copied = writeTextMock.mock.calls[0][0] as string;
    expect(copied).toContain(PLACEHOLDER);
    expect(copied).not.toContain("Cached EN translation.");
  });
});
