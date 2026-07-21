# Tauri Windowing & the Floating / Non-Activating Panel

WhisperPilot's identity is an always-on-top bubble that expands into a chat panel, on macOS only (primary target; Windows support deferred per project scope).

## Baseline floating window (cross-platform, no native code)

Configure in `tauri.conf.json` (or at runtime via `WebviewWindowBuilder`):

- `alwaysOnTop: true`
- `decorations: false` (frameless bubble)
- `transparent: true` (rounded/transparent bubble; requires `macOSPrivateApi: true` on macOS for true transparency)
- `skipTaskbar: true` (Windows) / accessory activation policy (macOS) so it behaves like a widget, not a normal app window
- `resizable: false` on the bubble, `resizable: true` on the expanded panel
- Drag via the JS `getCurrentWindow().startDragging()` on a pointer-down handler, or CSS `data-tauri-drag-region` on the drag surface

Show above fullscreen apps / on all spaces: set the window to join all spaces / be a utility-style window. On macOS this needs the panel behavior below; the cross-platform `setVisibleOnAllWorkspaces`-style flags cover the simpler cases.

## The non-activating panel (per-OS native shim, post-MVP)

"Type in the bubble without your editor losing focus" is **not** available from plain Tauri window config — clicking a normal window takes key focus.

- **macOS:** convert the `NSWindow` backing the Tauri webview into a non-activating `NSPanel` (`.nonactivatingPanel` style mask, `.canJoinAllSpaces`/`.fullScreenAuxiliary` collection behavior). Prior art exists (e.g. `dannysmith/tauri-template`'s Quick Pane). Implement in Rust via `objc2`, gated behind `#[cfg(target_os = "macos")]`.
- **Windows:** use the `WS_EX_NOACTIVATE` extended window style via the `windows` crate, gated behind `#[cfg(target_os = "windows")]`.

Keep this behind one Rust function (e.g. `make_panel(window)`) with a no-op/empty default, so React never knows the difference.

## Interaction: drag regions & click-vs-drag

These are recurring bug sources because they are invisible to jsdom. Treat them as standing invariants:

- **The whole window-chrome surface is a drag region.** Mark the entire header (and the collapsed bubble) with `data-tauri-drag-region`, including its non-interactive children (title, status text, icon). A header that is only partially draggable is a bug.
- **Interactive controls sitting on a drag region need click-vs-drag discrimination.** A button on a drag surface (e.g. the header minimize mark) gets the same pointer stream as the drag: record the press origin on `mousedown` and ignore the click if the pointer moved past the threshold. **Reuse the existing `wasDragged` helper (`src/state/drag.ts`); do not re-derive it.** (Note: `src/` directory structure is pending scaffold — this helper will be created during the application shell task.)
- **Auto-sizing elements must react to container/window resize,** not only to their own value changes — e.g. an auto-grow textarea recomputes on `window` `resize`, not just on input change.
- These behaviors — dragging, click-vs-drag, layout sizing, scroll/pin, WebKit rendering — are **runtime-only**: jsdom cannot verify them. They must be checked in the real Tauri window (WKWebView) or the WebKit harness. See `AGENTS.md` → Quality Gates.

## Rules
- The bubble window config lives in Rust/`tauri.conf.json`, never re-implemented in React.
- Any window-level behavior that differs by OS goes behind a `#[cfg(...)]` Rust shim with a documented common interface.
- Don't block the MVP on the non-activating panel — a normal always-on-top window is acceptable for v1; the panel upgrade is post-MVP (see `docs/idea.md`).
