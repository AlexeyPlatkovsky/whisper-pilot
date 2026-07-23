# WKWebView Compatibility

WhisperPilot's platform scope is defined by
`.claude/conventions/react-tauri/desktop-platform-scope.md`. This convention covers only
front-end compatibility with the macOS WKWebView (WebKit) used by Tauri; a Chromium,
Firefox, or Vite development browser is not equivalent evidence.

## Compatibility rules

- Before introducing a browser API, CSS feature, or media behavior that is absent from the
  existing codebase, confirm it in the macOS 13+ WKWebView target defined by
  `desktop-platform-scope.md`. A primary WebKit source must establish availability on that
  minimum target; a real-window result on a newer macOS version does not establish it. When
  minimum-target support is unproven, provide a capability-detected fallback. Record the tested
  macOS and Safari/WebKit versions in runtime evidence. Do not infer support from Chromium behavior.
- Treat rendering affected by fonts, scrolling, `backdrop-filter`, transparency, date and
  internationalization formatting, clipboard access, or media playback as WKWebView-sensitive
  until verified in a real Tauri window.
- Use Tauri APIs for OS capabilities (filesystem, shell, notifications, and native dialogs).
  For clipboard access, use the approved Tauri capability when the browser Clipboard API does
  not meet the required WKWebView behavior. Follow
  `.claude/conventions/react-tauri/tauri-ipc-permissions.md` for every command, plugin, and
  permission change; this convention does not authorize a plugin or permission.
- Do not add vendor-specific scrollbar CSS solely to hide scrollbars. If a design requirement
  calls for custom scrollbar behavior, verify keyboard and assistive-technology usability and
  the real-window appearance.

## Required verification

- Browser tests are sufficient only for behavior they execute independently of WebKit and
  native-window rendering.
- Run the affected UI in the actual macOS Tauri development window when a change touches a
  WKWebView-sensitive behavior, native dialog or clipboard handoff, window transparency, or
  visual layout that depends on WebKit rendering.
- Record the checked behavior and outcome in the task's required validation evidence. Do not
  claim that a development-browser result verifies a WKWebView-specific behavior.
