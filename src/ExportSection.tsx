import { useEffect, useState } from "react";
import { getSettings, setSetting } from "./ipc";
import type { ExportFileType } from "./export";

const FILE_TYPES: { value: ExportFileType; label: string }[] = [
  { value: "plain_text", label: "Plain text (.txt)" },
  { value: "markdown", label: "Markdown (.md)" },
];

export function ExportSection() {
  const [fileType, setFileType] = useState<ExportFileType | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [changeError, setChangeError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getSettings()
      .then((s) => {
        if (!cancelled) setFileType(s.export_file_type as ExportFileType);
      })
      .catch((e) => {
        if (!cancelled) setLoadError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function handleChange(value: ExportFileType) {
    const previous = fileType;
    setChangeError(null);
    setFileType(value);
    try {
      await setSetting("export_file_type", value);
    } catch (e) {
      if (previous) setFileType(previous);
      setChangeError(String(e));
    }
  }

  if (loadError) return <p role="alert">{loadError}</p>;
  if (!fileType) return <p>Loading…</p>;

  return (
    <>
      <fieldset className="settings-radio-group">
        <legend>Export file type</legend>
        {FILE_TYPES.map((t) => (
          <label key={t.value}>
            <input
              type="radio"
              name="export-file-type"
              value={t.value}
              checked={fileType === t.value}
              onChange={() => handleChange(t.value)}
            />
            {t.label}
          </label>
        ))}
      </fieldset>
      {changeError && <p role="alert">{changeError}</p>}
    </>
  );
}
