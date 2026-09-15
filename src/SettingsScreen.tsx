import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AiModelsSection } from "./AiModelsSection";
import { AppearanceSection } from "./AppearanceSection";
import { AppLanguageSection } from "./AppLanguageSection";
import { CloudProviderSection } from "./CloudProviderSection";
import { ExportSection } from "./ExportSection";
import { AppLogo, Icon, type IconName } from "./Icon";
import { RecorderSettingsSection } from "./RecorderSettingsSection";

type SectionId =
  | "ai-models"
  | "appearance"
  | "app-language"
  | "export"
  | "cloud-provider"
  | "recorder";

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
  {
    id: "export",
    label: "Export",
    title: "Export",
    icon: "download",
  },
  {
    id: "cloud-provider",
    label: "Cloud provider",
    title: "Cloud Provider",
    icon: "cloud",
  },
  {
    id: "recorder",
    label: "Recorder",
    title: "Recorder",
    icon: "mic",
  },
];

function SectionContent({
  id,
  cloudProviderLocked,
}: {
  id: SectionId;
  cloudProviderLocked: boolean;
}) {
  switch (id) {
    case "ai-models":
      return <AiModelsSection />;
    case "appearance":
      return <AppearanceSection />;
    case "app-language":
      return <AppLanguageSection />;
    case "export":
      return <ExportSection />;
    case "cloud-provider":
      return <CloudProviderSection locked={cloudProviderLocked} />;
    case "recorder":
      return <RecorderSettingsSection />;
  }
}

export function SettingsScreen({
  onClose,
  cloudProviderLocked = false,
}: {
  onClose: () => void;
  cloudProviderLocked?: boolean;
}) {
  const [section, setSection] = useState<SectionId>("ai-models");
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const closeRef = useRef<HTMLButtonElement | null>(null);
  const openerRef = useRef<HTMLElement | null>(
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null,
  );
  const hasOpener =
    openerRef.current !== null && openerRef.current !== document.body;

  useEffect(() => {
    return () => {
      if (hasOpener) openerRef.current?.focus();
    };
  }, [hasOpener]);

  useEffect(() => {
    function handleWindowEscape(event: KeyboardEvent) {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      // Child dialogs handle Escape themselves.  Their handlers stop
      // propagation; this guard also covers a native dialog implementation
      // that cannot do so before the window listener runs.
      if (document.querySelector('[role="alertdialog"]')) return;
      const nested = document.querySelectorAll(
        '[role="dialog"][aria-modal="true"]',
      );
      if (nested.length > 1) return;
      event.preventDefault();
      onClose();
    }
    window.addEventListener("keydown", handleWindowEscape);
    return () => window.removeEventListener("keydown", handleWindowEscape);
  }, [onClose]);

  useLayoutEffect(() => {
    if (hasOpener) closeRef.current?.focus();
  }, [hasOpener]);

  const active = SECTIONS.find((s) => s.id === section)!;

  function handleKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    // A nested sheet/confirmation owns its own Escape and focus cycle.
    const nestedDialog = (event.target as HTMLElement).closest(
      '[role="alertdialog"], [role="dialog"]',
    );
    if (nestedDialog && nestedDialog !== dialogRef.current) return;

    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key !== "Tab") return;
    const controls = Array.from(
      dialogRef.current?.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ) ?? [],
    );
    if (controls.length === 0) return;
    const first = controls[0];
    const last = controls[controls.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div
      ref={dialogRef}
      className="settings-screen"
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
      onKeyDown={handleKeyDown}
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
              ref={closeRef}
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
          <SectionContent
            id={section}
            cloudProviderLocked={cloudProviderLocked}
          />
        </div>
      </div>
    </div>
  );
}
