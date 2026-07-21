# Desktop Platform Scope

WhisperPilot targets **macOS only** (macOS 13+). It is not a Windows, Linux, iOS, Android, or web product unless the user explicitly changes platform scope.

## App Assets

- Keep app icon source assets in `src-tauri/icons/` for regenerating desktop outputs.
- Keep only macOS-relevant Tauri outputs:
  - `icon.icns` — macOS app icon
  - Shared PNGs referenced by `src-tauri/tauri.conf.json`
- Remove any generated `src-tauri/icons/ios/`, `src-tauri/icons/android/`, or Windows `icon.ico` / `StoreLogo.png` directories — WhisperPilot does not target those platforms.
- Verify every icon path referenced by `src-tauri/tauri.conf.json` exists.

## Tauri Configuration

- `bundle.targets` should be limited to `["dmg", "app"]` (macOS targets only) — do not include `msi`, `nsis`, `deb`, or `appimage`.
- `tauri.conf.json` window config should use macOS-specific settings: `transparent`, `decorations: false` on macOS 14+ for rounded corners, `macOSPrivateApi: true` for ScreenCaptureKit.

## Design

- Design and test window layouts against macOS desktop.
- Do not optimize layouts for phone, tablet, or touch breakpoints.
- The primary window is a resizable standard macOS window (not a floating bubble in MVP).
