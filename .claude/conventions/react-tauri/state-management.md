# State Management

Choose state by what owns the data. The current application uses React state and typed IPC wrappers; it does not include Zustand or TanStack Query.

## 1. `useState` / `useReducer` — component-local
Ephemeral UI state that no other component needs: input text, open/closed toggles, hover state.

## 2. Shared application state
Keep shared state in the smallest common React owner and pass it through explicit props or a narrow context. Introduce a dedicated state library only when the routed design identifies a concrete cross-screen ownership or synchronization problem; document and test the new boundary in that task.

## 3. Rust-owned or persisted state
Meeting records, settings, model availability, transcription results, and other Rust-owned data are obtained through `src/ipc.ts`. Components must represent loading and error states, avoid stale updates after unmount or a replacement request, and refresh the local view after a mutation when the user-visible state changes.

## Anti-patterns (always findings)
- Bypassing the `src/ipc.ts` wrapper boundary; see `.claude/conventions/react-tauri/tauri-ipc-permissions.md` for the command and permission rule.
- Applying an older async response after the view has unmounted or a newer request replaced it.
- Keeping a duplicated meeting, settings, or model value without a defined refresh/update path.
- Adding Zustand or TanStack Query solely because this convention mentions state management.
