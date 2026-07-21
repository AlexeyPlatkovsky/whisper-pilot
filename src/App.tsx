import { useMemo, useState } from "react";
import {
  openFileDialog,
  transcribeFile,
  saveTextDialog,
  type Segment,
} from "./ipc";

type Status =
  | { kind: "idle" }
  | { kind: "transcribing"; file: string }
  | { kind: "error"; message: string };

function formatTime(ms: number): string {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function App() {
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const [fileName, setFileName] = useState<string | null>(null);
  const [segments, setSegments] = useState<Segment[]>([]);

  const transcriptText = useMemo(
    () => segments.map((s) => s.text).join("\n"),
    [segments],
  );

  async function handleAddFile() {
    try {
      const path = await openFileDialog();
      if (!path) return;
      const shortName = path.split("/").pop() ?? path;
      setStatus({ kind: "transcribing", file: shortName });
      setSegments([]);
      setFileName(shortName);
      const result = await transcribeFile(path, "ru");
      setSegments(result.segments);
      setFileName(result.file_name);
      setStatus({ kind: "idle" });
    } catch (e) {
      setStatus({ kind: "error", message: String(e) });
    }
  }

  function editSegment(index: number, text: string) {
    setSegments((prev) =>
      prev.map((s, i) => (i === index ? { ...s, text } : s)),
    );
  }

  async function handleSave() {
    const base = (fileName ?? "transcript").replace(/\.[^.]+$/, "");
    await saveTextDialog(transcriptText, `${base}.txt`);
  }

  const busy = status.kind === "transcribing";

  return (
    <div className="app">
      <header className="topbar">
        <h1>MFUPilot</h1>
        <div className="actions">
          <button className="primary" onClick={handleAddFile} disabled={busy}>
            Добавить файл
          </button>
          <button onClick={handleSave} disabled={busy || segments.length === 0}>
            Сохранить
          </button>
        </div>
      </header>

      {status.kind === "error" && (
        <div className="banner error">{status.message}</div>
      )}

      {busy && (
        <div className="banner">
          Транскрибирую <b>{status.file}</b>… это может занять несколько минут.
        </div>
      )}

      {!busy && segments.length === 0 && status.kind !== "error" && (
        <div className="empty">
          <p>Добавьте аудио или видео файл, чтобы получить транскрипцию.</p>
        </div>
      )}

      {segments.length > 0 && (
        <main className="transcript">
          {fileName && <div className="filename">{fileName}</div>}
          {segments.map((seg, i) => (
            <div className="segment" key={i}>
              <span className="time">{formatTime(seg.start_ms)}</span>
              <textarea
                className="text"
                value={seg.text}
                rows={1}
                onChange={(e) => editSegment(i, e.target.value)}
              />
            </div>
          ))}
        </main>
      )}
    </div>
  );
}
