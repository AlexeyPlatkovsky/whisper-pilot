import { useCallback, useEffect, useRef, useState } from "react";
import {
  getCloudProviderConfig,
  getSettings,
  setSetting,
  type CloudProviderConfiguration,
} from "./ipc";

/** View preferences are best-effort and never block capture. */
export function useStreamingPreferences(settingsOpen: boolean) {
  const [mfuPanelVisible, setMfuPanelVisible] = useState(true);
  const [cloudConfiguration, setCloudConfiguration] =
    useState<CloudProviderConfiguration | null>(null);
  const cloudRequestRef = useRef(0);

  useEffect(() => {
    getSettings()
      .then((settings) =>
        setMfuPanelVisible(settings.mfu_panel_streaming ?? true),
      )
      .catch(() => setMfuPanelVisible(true));
  }, []);

  const refreshCloudConfiguration = useCallback(async () => {
    const request = ++cloudRequestRef.current;
    try {
      const configuration = await getCloudProviderConfig();
      if (request === cloudRequestRef.current)
        setCloudConfiguration(configuration);
      return configuration;
    } catch {
      if (request === cloudRequestRef.current) setCloudConfiguration(null);
      return null;
    }
  }, []);

  useEffect(() => {
    if (!settingsOpen) void refreshCloudConfiguration();
  }, [refreshCloudConfiguration, settingsOpen]);

  const handleToggleMfuPanel = useCallback((next: boolean) => {
    setMfuPanelVisible(next);
    void (async () => {
      try {
        await setSetting("mfu_panel_streaming", next ? "true" : "false");
      } catch {
        // Persistence is best-effort; the visible preference already changed.
      }
    })();
  }, []);

  return {
    mfuPanelVisible,
    cloudConfiguration,
    refreshCloudConfiguration,
    handleToggleMfuPanel,
  };
}
