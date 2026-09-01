import { useEffect, useRef, useState } from "react";
import {
  getCloudProviderConfig,
  removeCloudProviderApiKey,
  saveCloudProviderApiKey,
  selectCloudProvider,
  verifyCloudProviderApiKey,
  type CloudProviderConfiguration,
  type CloudProviderId,
} from "./ipc";
import { Icon } from "./Icon";

function verificationErrorMessage(error: unknown): string {
  const detail = error instanceof Error ? error.message : String(error);
  if (detail.includes("OpenAI API key cannot access GPT Live Transcribe")) {
    return "This OpenAI key cannot access GPT Live Transcribe in its project.";
  }
  return "Unable to verify this API key. Check the key, provider access, and network.";
}

export function CloudProviderSection({ locked = false }: { locked?: boolean }) {
  const [configuration, setConfiguration] =
    useState<CloudProviderConfiguration | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [editingProvider, setEditingProvider] =
    useState<CloudProviderId | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [sheetError, setSheetError] = useState<string | null>(null);
  const [verified, setVerified] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const sheetRef = useRef<HTMLFormElement>(null);
  const openerRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    let cancelled = false;
    void getCloudProviderConfig().then(
      (next) => {
        if (!cancelled) setConfiguration(next);
      },
      () => {
        if (!cancelled) setError("Unable to load Cloud Provider settings.");
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  function closeSheet() {
    setEditingProvider(null);
    setApiKey("");
    setSheetError(null);
    setVerified(false);
    openerRef.current?.focus();
  }

  useEffect(() => {
    if (!editingProvider) return;
    inputRef.current?.focus();
  }, [editingProvider]);

  useEffect(() => {
    if (!editingProvider) return;
    function trapSheetFocus(event: KeyboardEvent) {
      if (event.key !== "Tab" || !sheetRef.current) return;
      const focusable = Array.from(
        sheetRef.current.querySelectorAll<HTMLElement>("button, input"),
      ).filter((element) => !element.hasAttribute("disabled"));
      if (focusable.length === 0) {
        event.preventDefault();
        sheetRef.current.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    window.addEventListener("keydown", trapSheetFocus, true);
    return () => window.removeEventListener("keydown", trapSheetFocus, true);
  }, [editingProvider]);

  function handleSheetKeyDown(event: React.KeyboardEvent<HTMLFormElement>) {
    if (event.key === "Escape" && !busy) {
      event.preventDefault();
      event.stopPropagation();
      closeSheet();
    }
  }

  async function handleProviderSelect(provider: CloudProviderId) {
    if (
      !configuration ||
      provider === configuration.selected_provider ||
      busy ||
      locked
    )
      return;
    setBusy(true);
    setError(null);
    try {
      setConfiguration(await selectCloudProvider(provider));
    } catch {
      setError("Unable to select Cloud Provider.");
    } finally {
      setBusy(false);
    }
  }

  async function handleSave(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!editingProvider || busy || locked || !verified) return;
    if (apiKey.trim().length === 0) {
      setSheetError("API key is required.");
      return;
    }
    setBusy(true);
    setSheetError(null);
    try {
      setConfiguration(await saveCloudProviderApiKey(editingProvider, apiKey));
      closeSheet();
    } catch {
      setSheetError("Unable to store API key in macOS Keychain.");
    } finally {
      setBusy(false);
    }
  }

  async function handleVerify() {
    if (!editingProvider || busy || locked) return;
    if (apiKey.trim().length === 0) {
      setSheetError("API key is required.");
      return;
    }
    setBusy(true);
    setSheetError(null);
    setVerified(false);
    try {
      await verifyCloudProviderApiKey(editingProvider, apiKey);
      setVerified(true);
    } catch (error) {
      setSheetError(verificationErrorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function handleRemove() {
    if (!editingProvider || busy || locked) return;
    setBusy(true);
    setSheetError(null);
    try {
      setConfiguration(await removeCloudProviderApiKey(editingProvider));
      closeSheet();
    } catch {
      setSheetError("Unable to remove API key from macOS Keychain.");
    } finally {
      setBusy(false);
    }
  }

  const editingStatus = configuration?.providers.find(
    (provider) => provider.id === editingProvider,
  );
  if (error)
    return (
      <div className="wp-notice wp-notice--error" role="alert">
        {error}
      </div>
    );
  if (!configuration) return <p>Loading Cloud Provider settings…</p>;

  return (
    <section
      className="model-section cloud-provider-section"
      aria-label="Cloud Provider settings"
    >
      <div className="section-header">
        <h4 className="section-title">Cloud transcription</h4>
        <p className="section-subtitle">
          Select the provider used for new Cloud streaming sessions.
        </p>
      </div>
      {locked && (
        <p className="cloud-provider-lock-note" role="status">
          Cloud provider settings are locked while streaming is live.
        </p>
      )}
      <ul
        className="model-list cloud-provider-list"
        role="radiogroup"
        aria-label="Cloud Provider"
      >
        {configuration.providers.map((provider) => (
          <li key={provider.id} className="model-row cloud-provider-row">
            <label className="cloud-provider-choice model-label">
              <input
                type="radio"
                name="cloud-provider"
                aria-label={`${provider.name} ${provider.model}`}
                checked={provider.id === configuration.selected_provider}
                onChange={() => void handleProviderSelect(provider.id)}
                disabled={busy || locked}
              />
              <span>{provider.name}</span>
            </label>
            <span className="model-size">{provider.model}</span>
            <span className="model-row-spacer" />
            {provider.configured && (
              <span
                className="model-status model-status--ready"
                title="API key configured"
              >
                <Icon name="check" size={14} />
                <span>API Key</span>
              </span>
            )}
            <button
              type="button"
              className="model-icon-btn"
              aria-label="Manage API key"
              title={`Manage ${provider.name} API key`}
              onClick={(event) => {
                openerRef.current = event.currentTarget;
                setEditingProvider(provider.id);
              }}
              disabled={busy || locked}
            >
              <Icon name="lock-keyhole" size={15} />
            </button>
          </li>
        ))}
      </ul>
      <p className="cloud-provider-security-note">
        <Icon name="lock-keyhole" size={16} />
        Your API keys are securely stored in macOS Keychain.
      </p>

      {editingProvider && editingStatus && (
        <div className="modal-overlay" role="presentation">
          <form
            ref={sheetRef}
            className="modal-panel cloud-key-sheet"
            tabIndex={-1}
            role="dialog"
            aria-modal="true"
            aria-label={`Manage ${editingStatus.name} API key`}
            onKeyDown={handleSheetKeyDown}
            onSubmit={(event) => void handleSave(event)}
          >
            <div className="modal-header">
              <h2 className="cloud-key-sheet-title">
                API key · {editingStatus.name}
              </h2>
              <button
                type="button"
                className="wp-icon-btn"
                aria-label="Close API key sheet"
                onClick={closeSheet}
                disabled={busy}
              >
                <Icon name="x" size={16} />
              </button>
            </div>
            <p className="cloud-key-sheet-copy">
              {editingStatus.configured
                ? "Replace the stored key or remove it from macOS Keychain."
                : "Saved securely in macOS Keychain."}
            </p>
            <label className="cloud-key-label">
              API key
              <input
                ref={inputRef}
                type="password"
                autoComplete="off"
                value={apiKey}
                onChange={(event) => {
                  setApiKey(event.target.value);
                  setVerified(false);
                }}
                disabled={busy || locked}
              />
            </label>
            <p className="cloud-key-sheet-caution">
              Verify access before saving. No audio is sent.
            </p>
            {verified && (
              <p className="cloud-key-verified" role="status">
                <Icon name="check" size={16} /> API key verified.
              </p>
            )}
            {sheetError && (
              <p className="cloud-key-error" role="alert">
                {sheetError}
              </p>
            )}
            <div className="cloud-key-sheet-actions">
              {editingStatus.configured && (
                <button
                  type="button"
                  className="model-icon-btn cloud-key-remove"
                  aria-label="Remove API key"
                  title="Remove API key"
                  onClick={() => void handleRemove()}
                  disabled={busy || locked}
                >
                  <Icon name="trash-2" size={15} />
                </button>
              )}
              <span className="cloud-key-sheet-spacer" />
              <button
                type="button"
                className="model-icon-btn model-icon-btn--accent"
                aria-label="Verify API key"
                title="Verify API key"
                onClick={() => void handleVerify()}
                disabled={busy || locked}
              >
                <Icon name="check" size={15} />
              </button>
              <button
                type="submit"
                className={`model-icon-btn model-icon-btn--accent cloud-key-save${
                  !verified ? " cloud-key-save--awaiting-verification" : ""
                }`}
                aria-label="Save API key"
                title="Save API key"
                disabled={busy || locked || !verified}
              >
                <Icon name="lock-keyhole" size={15} />
              </button>
            </div>
          </form>
        </div>
      )}
    </section>
  );
}
