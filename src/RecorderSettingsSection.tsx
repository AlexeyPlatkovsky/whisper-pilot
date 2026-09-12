import { useEffect, useState } from "react";
import {
  getRecorderShortcutStatus,
  getSettings,
  setRecorderShortcut,
} from "./ipc";

const DEFAULT_SHORTCUT = "Control+Option+Space";

export function RecorderSettingsSection() {
  const [value, setValue] = useState(DEFAULT_SHORTCUT);
  const [saved, setSaved] = useState(DEFAULT_SHORTCUT);
  const [message, setMessage] = useState<string | null>(null);
  const [active, setActive] = useState(false);

  async function refreshStatus() {
    try {
      const status = await getRecorderShortcutStatus();
      setActive(status.active);
      if (status.error) setMessage(status.error);
    } catch (error) {
      setActive(false);
      setMessage(String(error));
    }
  }

  useEffect(() => {
    void getSettings().then((settings) => {
      const shortcut = settings.recorder_shortcut ?? DEFAULT_SHORTCUT;
      setValue(shortcut);
      setSaved(shortcut);
    });
    void refreshStatus();
  }, []);

  async function save() {
    setMessage(null);
    try {
      const settings = await setRecorderShortcut(value);
      const shortcut = settings.recorder_shortcut ?? value;
      setValue(shortcut);
      setSaved(shortcut);
      setMessage("Recorder shortcut saved.");
      await refreshStatus();
    } catch (error) {
      setValue(saved);
      setMessage(String(error));
    }
  }

  return (
    <section
      className="settings-section"
      aria-labelledby="recorder-shortcut-heading"
    >
      <h4 id="recorder-shortcut-heading">Global recording shortcut</h4>
      <p className="settings-description">
        Use a modifier and one letter, number, function key, or Space. A
        conflicting shortcut is rejected and the previous shortcut stays active.
      </p>
      <p role="status">Shortcut status: {active ? "Active" : "Disabled"}</p>
      <label className="settings-field">
        Shortcut
        <input
          aria-label="Recorder global shortcut"
          value={value}
          onChange={(event) => setValue(event.target.value)}
          placeholder={DEFAULT_SHORTCUT}
        />
      </label>
      <button
        type="button"
        className="wp-btn wp-btn--primary"
        onClick={() => void save()}
        disabled={active && value.trim() === saved}
      >
        {active ? "Save shortcut" : "Retry shortcut"}
      </button>
      {message && <p role="status">{message}</p>}
    </section>
  );
}
