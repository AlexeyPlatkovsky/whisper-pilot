# Testing Pyramid Gap Analysis

Date: 2026-07-22

## Current Pyramid Shape

```
                     ┌──────────┐
                     │ 1 (E2E)  │  pipeline.rs — #[ignore], opt-in only
                    ┌┴──────────┴┐
                    │   ZERO     │  Controller / IPC bridge layer
                  ┌─┴────────────┴─┐
                  │      27        │  Component tests (RTL, IPC mocked)
               ┌──┴───────────────┴──┐
               │    31 unit tests    │  9 frontend pure + 22 Rust
              └──────────────────────┘
```

## Inventory

### Rust — Unit Tests (22 total)

| File | Count | Status |
|---|---|---|
| `src-tauri/src/settings.rs` | 8 | Covered — defaults, corruption, persistence, validation, rejection |
| `src-tauri/src/transcribe.rs` | 2 | Covered — model path resolution (override vs default) |
| `src-tauri/src/models.rs` | 13 | Covered — download verify, SHA mismatch, network failure, progress fractions, delete, catalog, asset paths |
| `src-tauri/src/audio.rs` | 0 | **Missing** |
| `src-tauri/src/lib.rs` | 0 | **Missing** |
| `src-tauri/src/error.rs` | 0 | **Missing** |

### Rust — Integration Tests

| File | Count | Status |
|---|---|---|
| `src-tauri/tests/pipeline.rs` | 1 | Covered — E2E file→ffmpeg→whisper→segments, `#[ignore]` by default |

### Frontend — Unit Tests (9)

| File | Count | Status |
|---|---|---|
| `src/i18n.test.ts` | 5 | Covered — string lookups, locale fallback |
| `src/theme.test.ts` | 4 | Covered — applyTheme dark/light/system |

### Frontend — Component Tests (~27)

| File | Count | Status |
|---|---|---|
| `src/App.test.tsx` | 17 | Covered — settings entry/close, model availability, i18n, timer, theme, file handling, save, sidebar, editing |
| `src/AiModelsSection.test.tsx` | 9 | Covered — download/delete buttons, confirmation, progress bar, errors, modal |
| `src/AppearanceSection.test.tsx` | 6 | Covered — theme load, selection, persistence, revert on failure |
| `src/AppLanguageSection.test.tsx` | 2 | Covered — language display, load error |
| `src/SettingsScreen.test.tsx` | 6 | Covered — tab nav, close, Escape, keyboard accessibility |

### Untested Source Files

| File | Reason |
|---|---|
| `src-tauri/src/audio.rs` (81 lines) | No test module. Contains `normalize_to_wav`, `decode_wav_16k_mono`, `load_samples` |
| `src-tauri/src/lib.rs` (~150 lines) | No test module. Contains 8 Tauri command handlers + `AppState` |
| `src-tauri/src/error.rs` (54 lines) | No test module. Contains `AppError` enum, `From<io::Error>`, custom `Serialize` |
| `src/ipc.ts` (78 lines) | Thin typed wrapper around `invoke()` — intentionally mocked at boundary |
| `src/Icon.tsx` (201 lines) | Pure-presentational SVG component |
| `src/App.tsx` utilities | `formatTime`, `formatRange`, `formatClock`, `formatDuration` — pure functions tested only indirectly via component DOM assertions |

---

## Gap 1 — `audio.rs` has zero tests (Critical)

File: `src-tauri/src/audio.rs:1-81`

Three functions, zero tests:

- **`normalize_to_wav(input: &Path) -> Result<PathBuf>`** — shells out to ffmpeg for audio extraction/resampling. Test surface: ffmpeg-not-found error, ffmpeg failure with stderr, successful conversion produces a valid temp file.
- **`decode_wav_16k_mono(path: &Path) -> Result<Vec<f32>>`** — decodes a WAV via hound. Branches on `SampleFormat::Int` (i16→f32 normalization) vs `SampleFormat::Float`. Validates channels=1 and sample_rate=16000. Test surface: valid 16-bit mono WAV, valid float WAV, wrong channel count, wrong sample rate, corrupted WAV header, empty WAV.
- **`load_samples(input: &Path) -> Result<Vec<f32>>`** — composition of normalize+decode with temp file cleanup. Test surface: verify temp file is deleted on both success and failure paths.

This is the highest-risk gap: audio ingestion is the first step of the transcription pipeline, and the format-branching logic in `decode_wav_16k_mono` has no coverage.

### Proposed approach

- Generate small in-memory WAV fixtures (16-bit mono, float mono, stereo, wrong rate) using `hound::WavWriter`.
- Test `normalize_to_wav` with a real short audio fixture (skip if ffmpeg unavailable, or mark `#[ignore]`).
- Test `decode_wav_16k_mono` with in-memory WAV fixtures (no ffmpeg dependency).
- Test `load_samples` composition and temp-file cleanup.

---

## Gap 2 — `lib.rs` Tauri command handlers have zero tests (Critical)

File: `src-tauri/src/lib.rs`

Eight registered Tauri commands — every one untested:

| Command | What it does | Integration test needed? |
|---|---|---|
| `open_file_dialog` | Opens native file picker, returns path | Needs `rfd` mocking or skip in CI |
| `transcribe_file` | Orchestrates load_model + transcribe_file + segment extraction | Yes — mock model or use test fixture |
| `save_text_dialog` | Opens native save dialog, writes file | Needs `rfd` mocking |
| `get_settings` | Reads settings from disk | Can test against temp dir |
| `set_setting` | Validates + writes setting | Can test against temp dir |
| `list_task_models` | Lists catalog model statuses | Can test against temp dir |
| `download_model` | Downloads + verifies a model | Can test with injected fetch |
| `delete_model` | Removes a model file | Can test against temp dir |

The frontend component tests **all mock `ipc.ts`**, so serialization, error propagation, and command orchestration across the IPC boundary are never exercised. A type mismatch between a TS `invoke()` call and the Rust command signature would not be caught until runtime.

### Proposed approach

- Extract `AppState` construction to accept injected directories (app support dir, model catalog) so tests don't depend on the real filesystem layout.
- Write integration tests in `src-tauri/tests/` that call commands through `tauri::test` or directly against the state:
  - `get_settings` / `set_setting` against a temp dir (easiest, high value).
  - `list_task_models` / `download_model` / `delete_model` against a temp dir with an injected fetch that writes controlled content (already proven in `models.rs` unit tests — move the orchestration test up one level).
  - `transcribe_file` with a controlled WAV fixture and a mock whisper context (or mark `#[ignore]` like pipeline.rs).
  - Dialog commands (`open_file_dialog`, `save_text_dialog`) are inherently UI-blocking — document as manual-only verification.

---

## Gap 3 — `App.test.tsx` has overly broad responsibilities (Moderate)

File: `src/App.test.tsx:1-341`

This single file mixes three test levels:

1. **Pure component tests** (correct layer): "renders Save button in English", "displays error when transcribeFile rejects", "does nothing when file dialog is cancelled".
2. **Orchestration/integration tests** (borderline but valuable): "closing Settings returns to workspace with transcript state intact", "re-enables Add-file after closing Settings once model downloaded". These require App + SettingsScreen + sections rendered together — genuine integration behavior, but tested at the wrong granularity (through DOM assertions rather than controller-level tests).
3. **Redundant coverage**: model availability warnings (duplicates AiModelsSection), theme application on mount (duplicates AppearanceSection), English strings (duplicates i18n).

A 341-line test file for a 504-line component is bloated. It acts as a catch-all, compensating for the missing controller-layer tests in Gap 2.

### Proposed approach

- Move cross-component orchestration tests (Settings open→close with state preservation) to a dedicated integration test file — or, better, back them with Rust-side command handler tests so the controller layer is verified without the UI.
- De-duplicate: remove App.test.tsx assertions that are identical to what section-level tests already cover.
- Extract the four timestamp formatting functions (`formatTime`, `formatRange`, `formatClock`, `formatDuration`) to a separate module with their own unit tests.

---

## Gap 4 — Timestamp formatting utilities are untested in isolation (Moderate)

File: `src/App.tsx` (inline)

Four pure functions defined inline in `App.tsx`:

| Function | Edge cases not tested |
|---|---|
| `formatTime(ms)` | ms=0, negative ms, very large values (>1 hour) |
| `formatRange(start, end)` | start > end, zero-length segments, very large ranges |
| `formatClock(totalSeconds)` | 0, negative, > 1 hour overflow |
| `formatDuration(ms)` | sub-second, fractional |

These are tested only as side-effects of DOM assertions in `App.test.tsx` (e.g., the timer test at lines 178–225 exercises `formatClock` indirectly). Pure functions with edge cases should have isolated unit tests at the base of the pyramid.

### Proposed approach

- Extract to `src/format.ts` or similar.
- Write unit tests covering: zero, negative, boundary, overflow, rounding.

---

## Gap 5 — Sidebar is under-tested (Moderate)

Only one test exists for the sidebar: toggle visibility. Zero tests for:

- `SAMPLE_MEETINGS` list rendering (empty state, populated state).
- Search/filter by meeting title.
- Meeting selection and visual feedback.
- Meeting status indicators (playing, etc.).

### Proposed approach

- At minimum, verify the sidebar renders the hardcoded `SAMPLE_MEETINGS` entries.
- Test search filtering by title substring.
- Test that clicking a row selects it.
- These are component-level tests, same layer as SettingsScreen.

---

## Gap 6 — `error.rs` has no unit tests (Minor)

File: `src-tauri/src/error.rs:1-54`

Untested:
- `AppError::Serialize` — custom `Serialize` impl that serializes as a plain string. A mismatch here would surface as a runtime Tauri error.
- `From<std::io::Error>` — trivial but untested.
- Error display strings — tested indirectly whenever a component test triggers an error via mock rejection, but the Rust-side formatting is never verified.

### Proposed approach

- Test that `serde_json::to_string(&AppError::FfmpegMissing)` produces `"ffmpeg is required but was not found on PATH"`.
- Test `From<io::Error>` conversion.
- Low priority; these are exercised indirectly but a direct test is cheap.

---

## Gap 7 — `ipc.ts` has no tests (Minor, Acceptable)

File: `src/ipc.ts:1-78`

All functions are thin typed wrappers around `tauri::invoke()`. The mocking strategy — mock `ipc.ts` at the boundary in every component test — is the correct pattern. No runtime test verifies that TS `invoke()` calls match Rust command signatures, but `tsc` catches obvious mismatches.

Verdict: documented gap, acceptable to leave untested.

---

## Gap 8 — `Icon.tsx` has no tests (Minor, Acceptable)

File: `src/Icon.tsx:1-201`

Pure-presentational SVG component. No logic, no state, no user interaction. Renders inline SVG paths. Visual-only verification.

Verdict: acceptable to leave untested.

---

## Correctly Layered Tests

These are well-placed and need no change:

| Test | Layer | Why correct |
|---|---|---|
| `i18n.test.ts` | Unit | Pure string lookup, no DOM, no IPC |
| `theme.test.ts` | Unit | Manipulates `document.documentElement.dataset` directly, no component |
| `settings.rs` (8 tests) | Unit | JSON persistence + validation against temp dirs |
| `transcribe.rs` (2 tests) | Unit | Path resolution logic, no Tauri runtime |
| `models.rs` (13 tests) | Unit | Injected fetch functions + temp dirs, no network |
| `AiModelsSection.test.tsx` | Component | Mocked IPC at `ipc.ts` boundary, tests UI behavior |
| `AppearanceSection.test.tsx` | Component | Mocked IPC, tests theme UI and optimistic update + revert |
| `AppLanguageSection.test.tsx` | Component | Mocked IPC, tests language display |
| `SettingsScreen.test.tsx` | Component | All IPC mocked, tests shell/nav/keyboard |
| `pipeline.rs` | E2E | Real ffmpeg + whisper, `#[ignore]`, opt-in |

---

## After-Mitigation Pyramid (Target)

```
                     ┌──────────┐
                     │ 1 (E2E)  │  pipeline.rs (unchanged)
                    ┌┴──────────┴┐
                    │   ~8-10    │  Tauri command handler integration tests
                  ┌─┴────────────┴─┐
                  │     ~35+       │  Component tests + sidebar + de-duplicated App
               ┌──┴────────────────┴──┐
               │      50+ unit tests  │  audio.rs added, format utils extracted, error.rs added
              └───────────────────────┘
```

---

## Prioritized Action Items

| # | Action | File(s) | Severity | Layer |
|---|---|---|---|---|
| 1 | Add unit tests for `audio.rs` (WAV decode, format branches, temp cleanup) | `src-tauri/src/audio.rs` | Critical | Unit |
| 2 | Add integration tests for Tauri command handlers (settings, models, transcribe orchestration) | `src-tauri/tests/commands.rs` (new) or inline in `src-tauri/src/lib.rs` | Critical | Integration |
| 3 | Extract timestamp formatters to a separate module with unit tests | `src/format.ts` (new), `src/format.test.ts` (new) | Moderate | Unit |
| 4 | De-duplicate `App.test.tsx` — remove assertions already covered by section-level tests | `src/App.test.tsx` | Moderate | Component |
| 5 | Add sidebar rendering and search tests | `src/App.test.tsx` or new `src/Sidebar.test.tsx` | Moderate | Component |
| 6 | Add error serialization unit tests | `src-tauri/src/error.rs` | Minor | Unit |
| 7 | Document `ipc.ts` and `Icon.tsx` as intentionally untested | `docs/testing.md` | Minor | Docs |
