import { SettingsScreen } from "./SettingsScreen";

/** Renders Settings in-place or over a live workspace without duplicating its lifecycle. */
export function SettingsLayer({
  open,
  overlay = false,
  cloudProviderLocked,
  onClose,
}: {
  open: boolean;
  overlay?: boolean;
  cloudProviderLocked: boolean;
  onClose: () => void;
}) {
  if (!open) return null;
  const settings = (
    <SettingsScreen
      cloudProviderLocked={cloudProviderLocked}
      onClose={onClose}
    />
  );
  return overlay ? (
    <div className="settings-overlay">{settings}</div>
  ) : (
    settings
  );
}
