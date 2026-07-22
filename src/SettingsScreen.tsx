import { useEffect, useState } from "react";
import { AiModelsSection } from "./AiModelsSection";
import { AppearanceSection } from "./AppearanceSection";
import { AppLanguageSection } from "./AppLanguageSection";

type SectionId = "ai-models" | "appearance" | "app-language";

const SECTIONS: { id: SectionId; label: string }[] = [
  { id: "ai-models", label: "AI models" },
  { id: "appearance", label: "Appearance" },
  { id: "app-language", label: "App language" },
];

function SectionContent({ id }: { id: SectionId }) {
  switch (id) {
    case "ai-models":
      return <AiModelsSection />;
    case "appearance":
      return <AppearanceSection />;
    case "app-language":
      return <AppLanguageSection />;
  }
}

export function SettingsScreen({ onClose }: { onClose: () => void }) {
  const [section, setSection] = useState<SectionId>("ai-models");

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <div className="settings-screen" role="dialog" aria-modal="true" aria-label="Settings">
      <div className="settings-header">
        <h2>Settings</h2>
        <button
          className="close-button"
          aria-label="Close settings"
          onClick={onClose}
        >
          ×
        </button>
      </div>
      <div className="settings-body">
        <div className="settings-nav" role="tablist" aria-label="Settings sections">
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              id={`settings-tab-${s.id}`}
              role="tab"
              aria-selected={s.id === section}
              aria-controls="settings-tabpanel"
              className={s.id === section ? "active" : undefined}
              onClick={() => setSection(s.id)}
            >
              {s.label}
            </button>
          ))}
        </div>
        <div
          id="settings-tabpanel"
          className="settings-content"
          role="tabpanel"
          aria-labelledby={`settings-tab-${section}`}
        >
          <SectionContent id={section} />
        </div>
      </div>
    </div>
  );
}
