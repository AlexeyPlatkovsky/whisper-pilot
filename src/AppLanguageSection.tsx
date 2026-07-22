import { useEffect, useState } from "react";
import { getSettings } from "./ipc";

export function AppLanguageSection() {
  const [uiLanguage, setUiLanguage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getSettings()
      .then((s) => {
        if (!cancelled) setUiLanguage(s.ui_language);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (error) return <p role="alert">{error}</p>;
  if (!uiLanguage) return <p>Loading…</p>;

  return (
    <fieldset className="settings-radio-group">
      <legend>Language</legend>
      <label>
        <input
          type="radio"
          name="app-language"
          checked={uiLanguage === "en"}
          readOnly
        />
        English
      </label>
    </fieldset>
  );
}
