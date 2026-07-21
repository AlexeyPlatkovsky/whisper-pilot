# Tauri IPC, Commands, `tauri-specta` & Permissions

## Commands (Rust → exposed to React)

- Define commands with `#[tauri::command]` in the Rust core; register them in the `invoke_handler`.
- Keep commands thin: validate input, call a plain Rust function/trait (unit-testable without Tauri), map the result/error to a serializable type.
- Long-running work (CLI subprocess) must be `async` and cancellable; never block the main thread.

## Type-safe bindings with `tauri-specta`

- Annotate commands and their argument/return types so `tauri-specta` can emit a TypeScript bindings file.
- The React side imports the generated `commands.xxx()` / typed `invoke` wrapper — **do not** hand-write `invoke<SomeType>("name")`, which silently lies about the return type.
- Regenerate bindings whenever a command signature changes; treat the generated file as build output, not hand-edited.

## Capabilities & permissions (Tauri v2)

- Tauri v2 is deny-by-default. Every command and plugin API the front-end uses must be enabled in `src-tauri/capabilities/*.json`.
- Scope plugin permissions tightly: e.g. `shell` plugin should allow only the specific CLI invocations the app needs, not arbitrary execution.
- A frontend call failing with a permission error is a missing-capability bug — fix the capability file, don't broaden permissions blindly.

## Rules
- Command handlers are thin adapters over testable Rust functions.
- All IPC types are generated or explicitly validated; never `any`.
- Every IPC/plugin call has a corresponding, minimally-scoped capability permission.
- Errors cross the IPC boundary as typed, serializable values that React can branch on (map to the adapter error taxonomy in `docs/idea.md` where relevant).
