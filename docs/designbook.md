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
- status colours: success green, error red, in-progress teal-blue, plus two
  values **derived here rather than extracted from the pencil file** —
  `--wp-attention` (dark amber, for a meeting with no source file, which the
  pencil design gives no colour) and `--wp-success-text` (a darker green for
  small status text, because the pencil `$success` falls below the 4.5:1
  contrast the accessibility convention requires at 12px). Fold both into
  `pencil/main_view.pen` on its next revision.
- one meeting status is shown in two places at once — the sidebar row's dot and
  the header status widget. Both resolve through `src/meetingStatus.ts` and are
  coloured by the `--tone-*` semantic variables via the `.wp-tone--*` classes,
  so a status has exactly one colour and one label wherever it appears. Each
  status's colour is user-configurable (Settings → Appearance → Status Colors,
  WP-88): the saved color is written to a per-status `--status-color-*`
  variable that a `.wp-status--*` class resolves ahead of the tone default.
- rounded corners 6–16px (`--wp-radius-*`)
- dense information layout, desktop-first, keyboard-first
- avoid mobile spacing

**Accepted accessibility exception — sidebar row status.** The sidebar row
prints no status text; the dot carries the status as colour, as a `title`
tooltip, and as the `aria-label` of its `role="img"`. This is a deliberate
density trade-off, chosen so the row shows only what distinguishes one meeting
from another. It deviates from
`.claude/conventions/react-tauri/accessibility.md` §"Status, transcript, and
colour", which asks for state as text and not colour alone: a screen-reader
user still hears the status, and a mouse user still gets the tooltip, but a
sighted keyboard-only user has colour alone for any row that is not the open
meeting — `title` tooltips are not reachable by keyboard or touch. The open
meeting's status is always spelled out in the header. Revisit by revealing the
label on `:hover, :focus-within` if the colour-only path proves confusing.

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
