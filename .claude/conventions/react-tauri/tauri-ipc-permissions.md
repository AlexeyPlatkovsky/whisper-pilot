# Tauri IPC, Commands, and Permissions

## Commands (Rust → exposed to React)

- Define commands with `#[tauri::command]` in `src-tauri/src/lib.rs` and register them in the `invoke_handler`.
- Keep command handlers thin: validate input, call a testable Rust function, and map results and errors to serializable values.
- CPU-intensive or synchronous file work must not run on the async reactor; use `tauri::async_runtime::spawn_blocking` for it. Use an already-async API directly only when it does not block the reactor. Cancellation is required only when the routed requirement calls for it.

## Type-safe front-end calls

- The current project centralizes typed `invoke` and event calls in `src/ipc.ts`; components import those wrappers rather than calling `invoke` directly.
- When a command shape changes, update its Rust registration and serializable DTOs together with the matching `src/ipc.ts` wrapper and TypeScript interface. Do not use `any` or duplicate command strings in UI components.
- This project does not currently use generated `tauri-specta` bindings. Do not introduce a binding generator as an incidental change; propose it through the normal architecture and routing process.

## Capabilities & permissions (Tauri v2)

- Tauri v2 capabilities are configured in `src-tauri/capabilities/*.json`. Check the active capability file when adding a plugin API, window permission, or a command outside the existing `core:default` scope.
- Scope plugin permissions tightly: e.g. `shell` plugin should allow only the specific CLI invocations the app needs, not arbitrary execution.
- A frontend call failing with a permission error is a missing-capability bug — fix the capability file, don't broaden permissions blindly.

## Rules
- Command handlers are thin adapters over testable Rust functions.
- All IPC types are explicitly represented in Rust and `src/ipc.ts`; never use `any`.
- Every IPC/plugin call has the applicable minimally scoped capability permission; do not broaden permissions without identifying the exact new API or command.
- Errors cross the IPC boundary as serializable values that React can present safely. Do not expose filesystem paths or internal diagnostic detail unless the product requirement permits it.
