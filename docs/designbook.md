# Product Design Book

## Design tokens — source of truth

**[`src/tokens.css`](../src/tokens.css) is the single source of truth for all
design tokens** (colours, spacing, radii, typography, and layout dimensions).

- These tokens are derived directly from the approved design at
  [`pencil/main_view.pen`](../pencil/main_view.pen) (its `variables` block).
  The pencil file is the **upstream design reference**; `src/tokens.css` is its
  code counterpart.
- `tokens.css` is imported before `styles.css` (see `src/main.tsx`), so the
  `--wp-*` custom properties are globally available to every stylesheet and
  component.
- **Rule:** never hardcode a colour, radius, spacing, font size, or layout
  dimension when a `--wp-*` token exists for it — consume `var(--wp-…)`.
- **Changing the design:** update `tokens.css` first (to match the pencil
  source), then let consumers pick up the change. When the pencil file changes,
  reconcile `tokens.css` with it.
- Light/dark **theming** is layered on top of the raw tokens as *semantic*
  variables (`--bg`, `--panel`, `--text`, `--accent`, …) in `styles.css`. The
  tokens describe the design's base (light) palette; the dark palette is derived
  in `styles.css`. An explicit Light/Dark choice (Settings → Appearance) wins
  over the OS scheme via `[data-theme]`.

## Visual language

Palette (see `tokens.css` for exact values):

- warm off-white canvas (`--wp-canvas-bg`) with white raised panels
  (`--wp-panel`)
- deep teal-navy text (`--wp-text-primary` / `-secondary` / `-label`)
- burnt-orange accent (`--wp-accent`) — the single primary accent
- status colours: success green, error red, in-progress teal-blue
- rounded corners 6–16px (`--wp-radius-*`)
- dense information layout, desktop-first, keyboard-first
- avoid mobile spacing

## Inspirations

- Raycast
- Linear
- CleanShot X
- Obsidian
- Cursor

## Rules

- max 2 accent colours (burnt orange + one status colour in context)
- no giant cards
- no unnecessary whitespace
- settings always searchable
