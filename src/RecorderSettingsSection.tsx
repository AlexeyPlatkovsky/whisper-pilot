import { useEffect, useState } from "react";
import {
  getRecorderShortcutStatus,
  getSettings,
  setBubbleAlwaysOnTop,
  setRecorderShortcut,
  setSetting,
} from "./ipc";

const DEFAULT_SHORTCUT = "Control+Option+Space";

export function RecorderSettingsSection() {
  const [value, setValue] = useState(DEFAULT_SHORTCUT);
  const [saved, setSaved] = useState(DEFAULT_SHORTCUT);
  const [message, setMessage] = useState<string | null>(null);
  const [active, setActive] = useState(false);
  const [alwaysOnTop, setAlwaysOnTop] = useState(false);
  const [language, setLanguage] = useState<"auto" | "ru" | "en">("auto");

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
      setAlwaysOnTop(settings.bubble_always_on_top ?? false);
      setLanguage(settings.recorder_language ?? "auto");
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
      <hr className="settings-divider" />
      <h4>Recorder speech language</h4>
      <p className="settings-description">
        Whisper supports Auto and mixed Russian/English. Qwen3-ASR requires
        Russian or English to be selected explicitly.
      </p>
      <label className="settings-field">
        Language
        <select
          aria-label="Recorder speech language"
          value={language}
          onChange={(event) => {
            const previous = language;
            const next = event.target.value as "auto" | "ru" | "en";
            setLanguage(next);
            setMessage(null);
            void setSetting("recorder_language", next).catch((error) => {
              setLanguage(previous);
              setMessage(String(error));
            });
          }}
        >
          <option value="auto">Auto / mixed</option>
          <option value="ru">Russian</option>
          <option value="en">English</option>
        </select>
      </label>
      <hr className="settings-divider" />
      <h4>Floating bubble</h4>
      <p className="settings-description">
        Collapse WhisperPilot into a 120 px status bubble. When Over All is
        enabled, the bubble appears above normal windows and on every Space,
        including fullscreen Spaces.
      </p>
      <label className="settings-toggle-row">
        <span>Over All</span>
        <input
          type="checkbox"
          aria-label="Keep Recorder bubble over all windows"
          checked={alwaysOnTop}
          onChange={(event) => {
            const next = event.target.checked;
            setAlwaysOnTop(next);
            setMessage(null);
            void setBubbleAlwaysOnTop(next).catch((error) => {
              setAlwaysOnTop(!next);
              setMessage(String(error));
            });
          }}
        />
      </label>
    </section>
  );
}
