# Tauri Windowing

WhisperPilot is a macOS desktop application with one resizable standard main window. The declared window configuration in `src-tauri/tauri.conf.json` is authoritative; do not introduce a floating bubble, non-activating panel, always-on-top behavior, or multi-window flow unless a routed product decision requires it.

## Current window contract

- Keep the main window's minimum size, resize behavior, decorations, and title-bar style aligned with `src-tauri/tauri.conf.json`.
- Configure window-wide behavior in Tauri configuration or a focused Rust window module. Do not recreate native window behavior in React.
- A change to dimensions, title-bar behavior, transparency, decorations, or additional windows requires real-window verification in the Tauri macOS app because browser and jsdom results are insufficient.
- If future work needs OS-specific behavior, keep it behind a Rust interface with `#[cfg(target_os = "macos")]`; define the product behavior and verification plan before implementation.

## Rules
- The main-window configuration lives in `src-tauri/tauri.conf.json`, never re-implemented in React.
- Any window-level behavior that differs by OS goes behind a `#[cfg(...)]` Rust shim with a documented common interface.
- Do not add bubble, panel, capture, or always-on-top assumptions to implementation or validation instructions without an approved product change.
