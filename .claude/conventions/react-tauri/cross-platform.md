# macOS / WKWebView Constraints

WhisperPilot targets **macOS only** (macOS 13+, Apple Silicon primary). Tauri uses **WKWebView** (WebKit) as the system WebView on macOS — not Chromium/WebView2.

A feature that works in your dev browser (Chrome/Firefox/Chromium) can fail in the packaged macOS app (WebKit). Treat WebKit as the authoritative target throughout development.

## Common WebKit divergences to watch

- Newer/experimental CSS or JS APIs that ship in Chromium before WebKit — avoid or polyfill; verify in the actual WKWebView.
- Font rendering, scrollbar styling, `backdrop-filter`, and transparency behavior are WebKit-specific — verify the window looks correct in the Tauri build, not just the browser.
- `Date`/`Intl`, clipboard, and media APIs can behave differently from Chromium — prefer Tauri plugins (`clipboard-manager`, etc.) over raw browser APIs for anything that touches the OS.
- CSS `scrollbar-width: none` and webkit-specific scrollbar selectors may be needed to hide scrollbars in the transcript panel.

## Rules

- Do not use a Chromium-only API without confirming WebKit support or providing a fallback.
- Prefer Tauri plugins over raw browser APIs for all OS-touching capabilities (clipboard, fs, shell, notifications).
- Test the UI in the actual `tauri dev` window (WKWebView), not only in a browser, before marking UI work complete.
- The WKWebView behavior is the ground truth — design to it, not to Chrome.
