import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SettingsScreen } from "./SettingsScreen";
import * as ipc from "./ipc";

// AiModelsSection and AppearanceSection (section content) talk to the
// Tauri IPC layer; mock it at the boundary so these shell/nav-level tests
// don't depend on their internals (covered by their own test files).
vi.mock("./ipc", () => ({
  listTaskModels: vi.fn(async () => []),
  downloadModel: vi.fn(),
  deleteModel: vi.fn(),
  onModelDownloadProgress: vi.fn(async () => () => {}),
  getSettings: vi.fn(async () => ({
    theme: "system",
    ui_language: "en",
    active_model_diarization: "none",
  })),
  setSetting: vi.fn(),
  getCloudProviderConfig: vi.fn(async () => ({
    selected_provider: "deepgram",
    providers: [
      {
        id: "deepgram",
        name: "Deepgram",
        model: "Nova-3",
        configured: false,
      },
      {
        id: "assemblyai",
        name: "AssemblyAI",
        model: "Universal-3.5 Pro",
        configured: false,
      },
      {
        id: "openai",
        name: "OpenAI",
        model: "GPT Live Transcribe",
        configured: false,
      },
    ],
  })),
  selectCloudProvider: vi.fn(),
  verifyCloudProviderApiKey: vi.fn(),
  saveCloudProviderApiKey: vi.fn(),
  removeCloudProviderApiKey: vi.fn(),
}));

const CLOUD_CONFIGURATION = {
  selected_provider: "deepgram" as const,
  providers: [
    {
      id: "deepgram" as const,
      name: "Deepgram",
      model: "Nova-3",
      configured: false,
    },
    {
      id: "assemblyai" as const,
      name: "AssemblyAI",
      model: "Universal-3.5 Pro",
      configured: false,
    },
    {
      id: "openai" as const,
      name: "OpenAI",
      model: "GPT Live Transcribe",
      configured: false,
    },
  ],
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function verifySheetKey(
  user: ReturnType<typeof userEvent.setup>,
  sheet: HTMLElement,
) {
  await user.click(
    within(sheet).getByRole("button", { name: "Verify API key" }),
  );
  await within(sheet).findByText("API key verified.");
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(ipc.getCloudProviderConfig).mockResolvedValue(CLOUD_CONFIGURATION);
  vi.mocked(ipc.verifyCloudProviderApiKey).mockResolvedValue();
});

describe("SettingsScreen", () => {
  it("selects the AI models section by default", () => {
    render(<SettingsScreen onClose={vi.fn()} />);

    expect(screen.getByRole("tab", { name: "AI models" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("switches the visible section when a nav item is clicked", async () => {
    const user = userEvent.setup();
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Appearance" }));

    expect(screen.getByRole("tab", { name: "Appearance" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("tab", { name: "AI models" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    expect(
      await screen.findByRole("group", { name: "Theme" }),
    ).toBeInTheDocument();
  });

  it("calls onClose when the close button is clicked", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<SettingsScreen onClose={onClose} />);

    await user.click(screen.getByRole("button", { name: "Close settings" }));

    expect(onClose).toHaveBeenCalledOnce();
  });

  it("calls onClose when Escape is pressed", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<SettingsScreen onClose={onClose} />);

    await user.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalledOnce();
  });

  it("reaches the close button and every section tab via Tab alone", async () => {
    const user = userEvent.setup();
    render(<SettingsScreen onClose={vi.fn()} />);

    const closeButton = screen.getByRole("button", { name: "Close settings" });
    const tabs = screen.getAllByRole("tab");

    await user.tab();
    expect(document.activeElement).toBe(closeButton);
    for (const tab of tabs) {
      await user.tab();
      expect(document.activeElement).toBe(tab);
    }
  });

  it("switches to the App language section", async () => {
    const user = userEvent.setup();
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "App language" }));

    expect(screen.getByRole("tab", { name: "App language" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("radio", { name: "English" })).toBeInTheDocument();
  });

  // WP-106 C-3: Cloud provider choice shows only fixed provider/model details
  // and whether a key exists; the key itself is never rendered.
  it("shows the approved Cloud Provider choices and Keychain status", async () => {
    const user = userEvent.setup();
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Cloud provider" }));

    expect(
      await screen.findByRole("heading", { name: "Cloud Provider" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("radio", { name: /Deepgram.*Nova-3/i }),
    ).toBeChecked();
    expect(
      screen.getByRole("radio", { name: /AssemblyAI.*Universal-3.5 Pro/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("radio", { name: /OpenAI.*GPT Live Transcribe/i }),
    ).toBeInTheDocument();
    expect(
      screen.getAllByRole("button", { name: "Manage API key" }),
    ).toHaveLength(3);
    expect(
      screen.getByText("Your API keys are securely stored in macOS Keychain."),
    ).toBeInTheDocument();
  });

  it("uses the compact local-AI row and icon-action treatment for Cloud providers", async () => {
    const user = userEvent.setup();
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Cloud provider" }));
    const choices = await screen.findByRole("radiogroup", {
      name: "Cloud Provider",
    });
    expect(choices).toHaveClass("model-list");
    expect(
      screen.getAllByRole("button", { name: "Manage API key" })[0],
    ).toHaveClass("model-icon-btn");

    await user.click(
      screen.getAllByRole("button", { name: "Manage API key" })[0],
    );
    const sheet = await screen.findByRole("dialog", {
      name: "Manage Deepgram API key",
    });
    expect(
      within(sheet).getByRole("button", { name: "Verify API key" }),
    ).toHaveTextContent("");
    expect(
      within(sheet).getByRole("button", { name: "Save API key" }),
    ).toHaveTextContent("");
  });

  // Cloud credentials must be verified before the UI permits a Keychain save;
  // no key is rendered after either action.
  it("verifies a masked API key before enabling its Keychain save", async () => {
    const user = userEvent.setup();
    const cloudIpc = ipc as typeof ipc & {
      saveCloudProviderApiKey: ReturnType<typeof vi.fn>;
    };
    cloudIpc.saveCloudProviderApiKey.mockResolvedValue({
      selected_provider: "deepgram",
      providers: [
        { id: "deepgram", name: "Deepgram", model: "Nova-3", configured: true },
        {
          id: "assemblyai",
          name: "AssemblyAI",
          model: "Universal-3.5 Pro",
          configured: false,
        },
        {
          id: "openai",
          name: "OpenAI",
          model: "GPT Live Transcribe",
          configured: false,
        },
      ],
    });
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Cloud provider" }));
    await user.click(
      (await screen.findAllByRole("button", { name: "Manage API key" }))[0],
    );

    const sheet = await screen.findByRole("dialog", {
      name: "Manage Deepgram API key",
    });
    const keyInput = within(sheet).getByLabelText("API key");
    expect(keyInput).toHaveAttribute("type", "password");
    expect(
      within(sheet).getByRole("button", { name: "Save API key" }),
    ).toBeDisabled();
    expect(
      within(sheet).getByRole("button", { name: "Save API key" }),
    ).toHaveClass("cloud-key-save--awaiting-verification");
    await user.click(
      within(sheet).getByRole("button", { name: "Verify API key" }),
    );
    expect(
      await within(sheet).findByText("API key is required."),
    ).toBeInTheDocument();
    await user.type(keyInput, "not-a-real-key");
    await user.click(
      within(sheet).getByRole("button", { name: "Verify API key" }),
    );
    expect(ipc.verifyCloudProviderApiKey).toHaveBeenCalledWith(
      "deepgram",
      "not-a-real-key",
    );
    expect(
      await within(sheet).findByText("API key verified."),
    ).toBeInTheDocument();
    await user.click(
      within(sheet).getByRole("button", { name: "Save API key" }),
    );

    expect(cloudIpc.saveCloudProviderApiKey).toHaveBeenCalledWith(
      "deepgram",
      "not-a-real-key",
    );
    expect(await screen.findByText("API Key")).toBeInTheDocument();
    expect(screen.queryByText("not-a-real-key")).not.toBeInTheDocument();
  });

  it("updates the selected provider through the radio cards", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.selectCloudProvider).mockResolvedValue({
      ...CLOUD_CONFIGURATION,
      selected_provider: "assemblyai",
    });
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Cloud provider" }));
    const assemblyAi = await screen.findByRole("radio", {
      name: /AssemblyAI.*Universal-3.5 Pro/i,
    });
    await user.click(assemblyAi);

    expect(ipc.selectCloudProvider).toHaveBeenCalledWith("assemblyai");
    await waitFor(() => expect(assemblyAi).toBeChecked());
  });

  it("keeps Save unavailable and explains a failed API-key verification", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.verifyCloudProviderApiKey).mockRejectedValue(
      new Error("OpenAI API key cannot access GPT Live Transcribe."),
    );
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Cloud provider" }));
    await user.click(
      (await screen.findAllByRole("button", { name: "Manage API key" }))[2],
    );
    const sheet = await screen.findByRole("dialog", {
      name: "Manage OpenAI API key",
    });
    await user.type(within(sheet).getByLabelText("API key"), "not-a-real-key");
    await user.click(
      within(sheet).getByRole("button", { name: "Verify API key" }),
    );

    expect(
      await within(sheet).findByText(
        "This OpenAI key cannot access GPT Live Transcribe in its project.",
      ),
    ).toBeInTheDocument();
    expect(
      within(sheet).getByRole("button", { name: "Save API key" }),
    ).toBeDisabled();
    expect(ipc.saveCloudProviderApiKey).not.toHaveBeenCalled();
  });

  it("locks Cloud provider changes while a Streaming session is live", async () => {
    const user = userEvent.setup();
    render(<SettingsScreen onClose={vi.fn()} cloudProviderLocked />);

    await user.click(screen.getByRole("tab", { name: "Cloud provider" }));
    expect(
      await screen.findByRole("radio", { name: /Deepgram.*Nova-3/i }),
    ).toBeDisabled();
    expect(
      screen.getAllByRole("button", { name: "Manage API key" })[0],
    ).toBeDisabled();
    expect(
      screen.getByText(
        "Cloud provider settings are locked while streaming is live.",
      ),
    ).toBeInTheDocument();
  });

  it("closes the sheet with Escape, restores focus, and removes a configured key", async () => {
    const user = userEvent.setup();
    const configured = {
      ...CLOUD_CONFIGURATION,
      providers: CLOUD_CONFIGURATION.providers.map((provider) =>
        provider.id === "deepgram"
          ? { ...provider, configured: true }
          : provider,
      ),
    };
    vi.mocked(ipc.getCloudProviderConfig).mockResolvedValue(configured);
    vi.mocked(ipc.removeCloudProviderApiKey).mockResolvedValue(
      CLOUD_CONFIGURATION,
    );
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Cloud provider" }));
    const manage = (
      await screen.findAllByRole("button", {
        name: "Manage API key",
      })
    )[0];
    await user.click(manage);
    expect(
      await screen.findByRole("button", { name: "Remove API key" }),
    ).toBeInTheDocument();
    const sheet = screen.getByRole("dialog", {
      name: "Manage Deepgram API key",
    });
    const closeSheet = within(sheet).getByRole("button", {
      name: "Close API key sheet",
    });
    const verify = within(sheet).getByRole("button", {
      name: "Verify API key",
    });
    await waitFor(() =>
      expect(within(sheet).getByLabelText("API key")).toHaveFocus(),
    );
    expect(
      Array.from(sheet.querySelectorAll<HTMLElement>("button, input"))
        .filter((element) => !element.hasAttribute("disabled"))
        .at(-1),
    ).toBe(verify);
    verify.focus();
    await user.keyboard("{Tab}");
    expect(closeSheet).toHaveFocus();
    await user.keyboard("{Shift>}{Tab}{/Shift}");
    expect(verify).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(
      screen.queryByRole("dialog", { name: "Manage Deepgram API key" }),
    ).not.toBeInTheDocument();
    expect(manage).toHaveFocus();

    await user.click(manage);
    await user.click(
      await screen.findByRole("button", { name: "Remove API key" }),
    );
    expect(ipc.removeCloudProviderApiKey).toHaveBeenCalledWith("deepgram");
    await waitFor(() =>
      expect(
        screen
          .getByRole("radio", { name: /Deepgram.*Nova-3/i })
          .closest(".model-row"),
      ).not.toHaveTextContent("API Key"),
    );
  });

  it("keeps the sheet open and explains Keychain failures", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.saveCloudProviderApiKey).mockRejectedValue(
      new Error("keychain unavailable"),
    );
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Cloud provider" }));
    await user.click(
      (await screen.findAllByRole("button", { name: "Manage API key" }))[0],
    );
    const sheet = await screen.findByRole("dialog", {
      name: "Manage Deepgram API key",
    });
    await user.type(within(sheet).getByLabelText("API key"), "not-a-real-key");
    await verifySheetKey(user, sheet);
    await user.click(
      within(sheet).getByRole("button", { name: "Save API key" }),
    );

    expect(
      await within(sheet).findByText(
        "Unable to store API key in macOS Keychain.",
      ),
    ).toBeInTheDocument();
  });

  // WP-106 C-2, concurrency boundary: a save in flight keeps focus inside
  // the modal even though its controls are disabled.
  it("keeps keyboard focus in the API-key sheet while a save is pending", async () => {
    const user = userEvent.setup();
    const save = deferred<typeof CLOUD_CONFIGURATION>();
    vi.mocked(ipc.saveCloudProviderApiKey).mockReturnValue(save.promise);
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Cloud provider" }));
    await user.click(
      (await screen.findAllByRole("button", { name: "Manage API key" }))[0],
    );
    const sheet = await screen.findByRole("dialog", {
      name: "Manage Deepgram API key",
    });
    await user.type(within(sheet).getByLabelText("API key"), "not-a-real-key");
    await verifySheetKey(user, sheet);
    await user.click(
      within(sheet).getByRole("button", { name: "Save API key" }),
    );
    await waitFor(() =>
      expect(ipc.saveCloudProviderApiKey).toHaveBeenCalledTimes(1),
    );

    await user.keyboard("{Tab}");
    expect(sheet.contains(document.activeElement)).toBe(true);

    await act(async () => {
      save.resolve(CLOUD_CONFIGURATION);
    });
  });

  // WP-106 C-2, state transition: a configured provider replaces its stored
  // credential through the same masked sheet and still renders status only.
  it("replaces an already configured API key without displaying either key", async () => {
    const user = userEvent.setup();
    const configured = {
      ...CLOUD_CONFIGURATION,
      providers: CLOUD_CONFIGURATION.providers.map((provider) =>
        provider.id === "deepgram"
          ? { ...provider, configured: true }
          : provider,
      ),
    };
    vi.mocked(ipc.getCloudProviderConfig).mockResolvedValue(configured);
    vi.mocked(ipc.saveCloudProviderApiKey).mockResolvedValue(configured);
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Cloud provider" }));
    await user.click(
      (await screen.findAllByRole("button", { name: "Manage API key" }))[0],
    );
    const sheet = await screen.findByRole("dialog", {
      name: "Manage Deepgram API key",
    });
    expect(sheet).toHaveTextContent("Replace the stored key");
    await user.type(within(sheet).getByLabelText("API key"), "not-a-real-key");
    await verifySheetKey(user, sheet);
    await user.click(
      within(sheet).getByRole("button", { name: "Save API key" }),
    );

    expect(ipc.saveCloudProviderApiKey).toHaveBeenCalledWith(
      "deepgram",
      "not-a-real-key",
    );
    expect(
      screen.queryByRole("dialog", { name: "Manage Deepgram API key" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("not-a-real-key")).not.toBeInTheDocument();
  });

  it("shows a non-secret error when Cloud Provider settings cannot load", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.getCloudProviderConfig).mockRejectedValue(
      new Error("settings unavailable"),
    );
    render(<SettingsScreen onClose={vi.fn()} />);

    await user.click(screen.getByRole("tab", { name: "Cloud provider" }));

    expect(
      await screen.findByText("Unable to load Cloud Provider settings."),
    ).toBeInTheDocument();
  });
});
