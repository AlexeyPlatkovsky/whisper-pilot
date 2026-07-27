import { useEffect, useState } from "react";
import { AiModelsSection } from "./AiModelsSection";
import { AppearanceSection } from "./AppearanceSection";
import { AppLanguageSection } from "./AppLanguageSection";
import { AppLogo, Icon, type IconName } from "./Icon";

type SectionId = "ai-models" | "appearance" | "app-language";

const SECTIONS: {
  id: SectionId;
  label: string;
  title: string;
  icon: IconName;
}[] = [
  { id: "ai-models", label: "AI models", title: "AI Models", icon: "cpu" },
  {
    id: "appearance",
    label: "Appearance",
    title: "Appearance",
    icon: "palette",
  },
  {
    id: "app-language",
    label: "App language",
    title: "App Language",
    icon: "globe",
  },
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

  const active = SECTIONS.find((s) => s.id === section)!;

  return (
    <div
      className="settings-screen"
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
    >
      <header className="wp-header" data-tauri-drag-region="deep">
        <div className="wp-header-lead" data-tauri-drag-region="deep">
          <div className="wp-header-left" data-tauri-drag-region="deep">
            <span
              className="wp-traffic-space"
              aria-hidden="true"
              data-tauri-drag-region
            />
            <AppLogo size={28} />
            <div className="wp-title-group" data-tauri-drag-region="deep">
              <h1 className="wp-title">Settings</h1>
            </div>
          </div>

          <div className="wp-action-group">
            <button
              type="button"
              className="wp-icon-btn"
              aria-label="Close settings"
              onClick={onClose}
            >
              <Icon name="x" size={16} />
            </button>
          </div>
        </div>
      </header>
      <div className="settings-body">
        <div
          className="settings-nav"
          role="tablist"
          aria-label="Settings sections"
        >
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
              <Icon name={s.icon} size={16} />
              <span>{s.label}</span>
            </button>
          ))}
        </div>
        <div
          id="settings-tabpanel"
          className="settings-content"
          role="tabpanel"
          aria-labelledby={`settings-tab-${section}`}
        >
          <h3 className="settings-tab-title">{active.title}</h3>
          <SectionContent id={section} />
        </div>
      </div>
    </div>
  );
}
