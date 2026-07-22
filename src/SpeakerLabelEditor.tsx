import { useState } from "react";

export function SpeakerLabelEditor({
  speakerId,
  label,
  onRename,
}: {
  speakerId: number;
  label: string;
  onRename: (speakerId: number, newLabel: string) => void;
}) {
  const [isEditing, setIsEditing] = useState(false);
  const [draft, setDraft] = useState(label);

  function startEditing() {
    setDraft(label);
    setIsEditing(true);
  }

  function commit() {
    const trimmed = draft.trim();
    if (trimmed && trimmed !== label) onRename(speakerId, trimmed);
    setIsEditing(false);
  }

  function cancel() {
    setDraft(label);
    setIsEditing(false);
  }

  if (isEditing) {
    return (
      <input
        type="text"
        className="wp-speaker-name-input"
        aria-label={`Rename ${label}`}
        value={draft}
        autoFocus
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") cancel();
        }}
      />
    );
  }

  return (
    <button
      type="button"
      className="wp-speaker-name wp-speaker-name-button"
      onClick={startEditing}
      aria-label={`Rename ${label}`}
    >
      {label}
    </button>
  );
}
