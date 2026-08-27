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

// WP-94: Streaming Copy/Export wiring for the paired original+translation
// renderer (src/export.ts's renderStreamingPaired / hasStreamingTranslations),
// mirroring the inline vi.mock idiom of StreamingView.translation.test.tsx
// (WP-93) and StreamingView.test.tsx's clipboard-spy setup.

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
  translateStreamingParagraph: vi.fn(async () => "Translated."),
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

const PARAGRAPH_A = makeWindows(4, { startIndex: 0 });
const PARAGRAPH_B = makeWindows(4, { startIndex: 4 });
const SOURCE_A = paragraphSourceText(PARAGRAPH_A);
const SOURCE_B = paragraphSourceText(PARAGRAPH_B);
const TWO_PARAGRAPHS = [...PARAGRAPH_A, ...PARAGRAPH_B];

const PLACEHOLDER = "[Not translated]";
const FAILED_PLACEHOLDER = "[Translation failed]";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
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

describe("StreamingView — paired Copy/Export when Live Translation is on (WP-94)", () => {
  it("@WP-94-happy-paired-export: Copy places one Original + target-language block per paragraph on the clipboard, in screen order", async () => {
    const user = setupUser();
    vi.mocked(ipc.translateStreamingParagraph).mockImplementation(
      (_session, paragraphKey) =>
        Promise.resolve(
          paragraphKey === 0 ? "English batch A." : "English batch B.",
        ),
    );
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);

    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledTimes(2),
    );
    await screen.findByText("English batch B.");

    await clickCopy(user);

    expect(writeTextMock).toHaveBeenCalledWith(
      [
        "Original:",
        SOURCE_A,
        "",
        "Русский:",
        "English batch A.",
        "",
        "Original:",
        SOURCE_B,
        "",
        "Русский:",
        "English batch B.",
      ].join("\n"),
    );
  });

  it("Export writes the same paired body wrapped in the session's Markdown title heading", async () => {
    const user = setupUser();
    vi.mocked(ipc.translateStreamingParagraph).mockResolvedValue(
      "English batch A.",
    );
    await openSessionWithWindows(user, PARAGRAPH_A);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    await screen.findByText("English batch A.");

    await clickExport(user);

    expect(ipc.saveTextDialog).toHaveBeenCalledWith(
      `# Standup\n\nOriginal:\n${SOURCE_A}\n\nРусский:\nEnglish batch A.\n`,
      "Standup.md",
    );
  });

  it("Copy and Export both render from the same paired text — the two IPC calls carry the identical body", async () => {
    const user = setupUser();
    vi.mocked(ipc.translateStreamingParagraph).mockResolvedValue(
      "English batch A.",
    );
    await openSessionWithWindows(user, PARAGRAPH_A);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    await screen.findByText("English batch A.");

    await clickCopy(user);
    await clickExport(user);

    const copied = writeTextMock.mock.calls[0][0] as string;
    const exported = vi.mocked(ipc.saveTextDialog).mock.calls[0][0];
    expect(exported).toBe(`# Standup\n\n${copied}\n`);
  });

  it("@WP-94-placeholder-untranslated: a paragraph still awaiting translation is exported with the not-translated placeholder, leaving the finished paragraph unaffected", async () => {
    const user = setupUser();
    const first = deferred<string>();
    const second = deferred<string>();
    vi.mocked(ipc.translateStreamingParagraph).mockImplementation(
      (_session, paragraphKey) =>
        paragraphKey === 0 ? first.promise : second.promise,
    );
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledTimes(1),
    );
    first.resolve("English batch A.");
    await screen.findByText("English batch A.");
    // Paragraph B's call is now in flight but unresolved — still "pending"/
    // "translating" in the translations map.

    await clickCopy(user);

    expect(writeTextMock).toHaveBeenCalledWith(
      [
        "Original:",
        SOURCE_A,
        "",
        "Русский:",
        "English batch A.",
        "",
        "Original:",
        SOURCE_B,
        "",
        "Русский:",
        PLACEHOLDER,
      ].join("\n"),
    );

    second.resolve("English batch B.");
  });

  it("@WP-94-placeholder-failed: a paragraph whose translation failed is exported with a distinct failed-translation placeholder, not the not-yet-translated wording", async () => {
    const user = setupUser();
    vi.mocked(ipc.translateStreamingParagraph).mockImplementation(
      (_session, paragraphKey) =>
        paragraphKey === 0
          ? Promise.reject(new Error("model unavailable"))
          : Promise.resolve("English batch B."),
    );
    await openSessionWithWindows(user, TWO_PARAGRAPHS);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    await screen.findByRole("button", { name: /Translation failed.*Retry/i });
    await screen.findByText("English batch B.");

    await clickCopy(user);

    expect(writeTextMock).toHaveBeenCalledWith(
      [
        "Original:",
        SOURCE_A,
        "",
        "Русский:",
        FAILED_PLACEHOLDER,
        "",
        "Original:",
        SOURCE_B,
        "",
        "Русский:",
        "English batch B.",
      ].join("\n"),
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
    await screen.findByText(source, {
      selector: ".wp-translation-text--mirrored",
    });

    await clickCopy(user);

    expect(writeTextMock).toHaveBeenCalledWith(
      ["Original:", source, "", "Русский:", source].join("\n"),
    );
    expect(ipc.translateStreamingParagraph).not.toHaveBeenCalled();
  });

  it("@WP-94-stale: a persisted translation whose source text no longer matches the current paragraph exports the original with the not-translated placeholder", async () => {
    const user = setupUser();
    vi.mocked(ipc.listStreamingTranslations).mockResolvedValue([
      {
        paragraph_key: 0,
        source_text: "stale text that no longer matches",
        translated_text: "Stale cached translation.",
      },
    ]);
    const pending = deferred<string>();
    vi.mocked(ipc.translateStreamingParagraph).mockReturnValue(pending.promise);
    await openSessionWithWindows(user, PARAGRAPH_A);
    const toggle = await findTranslationSwitch();
    await user.click(toggle);
    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledWith(
        1,
        0,
        "ru",
        SOURCE_A,
        undefined,
      ),
    );

    await clickCopy(user);

    expect(writeTextMock).toHaveBeenCalledWith(
      ["Original:", SOURCE_A, "", "Русский:", PLACEHOLDER].join("\n"),
    );
    expect(writeTextMock.mock.calls[0][0]).not.toContain(
      "Stale cached translation.",
    );
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
                paragraph_key: 0,
                source_text: SOURCE_A,
                translated_text: "Cached EN translation.",
              },
            ]
          : [],
    );
    const pending = deferred<string>();
    vi.mocked(ipc.translateStreamingParagraph).mockReturnValue(pending.promise);
    await openSessionWithWindows(user, PARAGRAPH_A);
    const toggle = await findTranslationSwitch(); // target language defaults to "ru"
    await user.click(toggle);
    await waitFor(() =>
      expect(ipc.listStreamingTranslations).toHaveBeenCalledWith(1, "ru"),
    );
    await waitFor(() =>
      expect(ipc.translateStreamingParagraph).toHaveBeenCalledWith(
        1,
        0,
        "ru",
        SOURCE_A,
        undefined,
      ),
    );

    await clickCopy(user);

    expect(writeTextMock).toHaveBeenCalledWith(
      ["Original:", SOURCE_A, "", "Русский:", PLACEHOLDER].join("\n"),
    );
    expect(writeTextMock.mock.calls[0][0]).not.toContain(
      "Cached EN translation.",
    );
  });
});
