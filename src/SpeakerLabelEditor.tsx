import { useEffect, useState } from "react";

export function SpeakerLabelEditor({
  speakerId,
  label,
  onRename,
  disabled = false,
}: {
  speakerId: number;
  label: string;
  onRename: (speakerId: number, newLabel: string) => void;
  disabled?: boolean;
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

  // A run can start while the rename input is open (e.g. re-transcribing the
  // active meeting mid-edit); the input has no `disabled` state of its own,
  // so force it closed without committing the half-typed draft.
  useEffect(() => {
    if (disabled) cancel();
  }, [disabled]);

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
      disabled={disabled}
    >
      {label}
    </button>
  );
}
