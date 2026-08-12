import { useEffect, useState } from "react";
import { getSettings, setSetting, type Settings } from "./ipc";
import { applyTheme, type Theme } from "./theme";
import { StatusColorsSection } from "./StatusColorsSection";

const THEMES: { value: Theme; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
];

export function AppearanceSection() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [changeError, setChangeError] = useState<string | null>(null);
  const theme = (settings?.theme ?? null) as Theme | null;

  useEffect(() => {
    let cancelled = false;
    getSettings()
      .then((s) => {
        if (!cancelled) setSettings(s);
      })
      .catch((e) => {
        if (!cancelled) setLoadError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function handleChange(value: Theme) {
    const previous = theme;
    setChangeError(null);
    setSettings((s) => (s ? { ...s, theme: value } : s));
    applyTheme(value);
    try {
      await setSetting("theme", value);
    } catch (e) {
      if (previous) {
        setSettings((s) => (s ? { ...s, theme: previous } : s));
        applyTheme(previous);
      }
      setChangeError(String(e));
    }
  }

  if (loadError) return <p role="alert">{loadError}</p>;
  if (!settings || !theme) return <p>Loading…</p>;

  return (
    <>
      <fieldset className="settings-radio-group">
        <legend>Theme</legend>
        {THEMES.map((t) => (
          <label key={t.value}>
            <input
              type="radio"
              name="theme"
              value={t.value}
              checked={theme === t.value}
              onChange={() => handleChange(t.value)}
            />
            {t.label}
          </label>
        ))}
      </fieldset>
      {changeError && <p role="alert">{changeError}</p>}
      <StatusColorsSection statusColorsRaw={settings.status_colors} />
    </>
  );
}
