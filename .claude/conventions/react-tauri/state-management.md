# State Management (Zustand / TanStack Query)

Three layers, chosen by what *owns* the data:

## 1. `useState` / `useReducer` — component-local
Ephemeral UI state that no other component needs: input text, open/closed toggles, hover state.

## 2. Zustand — global UI state
Shared, app-owned UI state that is **not** fetched: active assistant selection, bubble expanded/collapsed, theme, current local session id. Small, synchronous, no cache semantics.

- One store per concern; select narrow slices to avoid needless re-renders (`useStore(s => s.field)`).
- Keep actions in the store; components call actions, they don't mutate.

## 3. TanStack Query — data owned by the Rust core / IPC
Anything that is fetched from or persisted by the Rust core: chat history from SQLite, the result of running an adapter, CLI availability/auth status.

- Wrap IPC calls (`invoke`/generated commands) in `useQuery`/`useMutation`.
- Use it for loading/error/stale states, caching, and invalidation instead of `useEffect` + `useState`.
- Sending a chat message = a mutation that, on success, invalidates the history query.

## Anti-patterns (always findings)
- Storing fetched/IPC data in `useState` or Zustand and manually syncing it.
- `useEffect(() => { invoke(...).then(setState) }, [])` for data that should be a `useQuery`.
- One giant global store holding everything, causing app-wide re-renders.
- Mutating Zustand state outside store actions.
