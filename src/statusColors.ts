// Single source of truth for the user-configurable status colors (WP-88).
// The 15 current semantic statuses across Meeting and Streaming each get one
// opaque color; the built-in mapping below is the default and the Reset-all
// target. Colors reach every status surface (header widgets, sidebar dots,
// spinners) as `--status-color-*` custom properties, consumed by the
// `.wp-status--*` classes in styles.css.

export interface StatusColorSpec {
  key: StatusColorKey;
  label: string;
  defaultColor: string;
}

const SPECS = [
  { key: "no-files", label: "No files", defaultColor: "#8A5F10" },
  { key: "ready", label: "Ready", defaultColor: "#5A7684" },
  { key: "transcribing", label: "Transcribing", defaultColor: "#176C8F" },
  { key: "diarizing", label: "Diarizing", defaultColor: "#176C8F" },
  { key: "crafting-notes", label: "Crafting notes", defaultColor: "#176C8F" },
  { key: "finished", label: "Finished", defaultColor: "#46704C" },
  { key: "error", label: "Error", defaultColor: "#B82B2F" },
  { key: "no-model", label: "No model", defaultColor: "#C65D2E" },
  { key: "unknown", label: "Unknown", defaultColor: "#5A7684" },
  { key: "starting", label: "Starting…", defaultColor: "#5A7684" },
  { key: "on-air", label: "On Air", defaultColor: "#B82B2F" },
  { key: "crafting-mfu", label: "Crafting MFU…", defaultColor: "#176C8F" },
  { key: "mfu-failed", label: "MFU Failed", defaultColor: "#B82B2F" },
  { key: "prettifying", label: "Prettifying…", defaultColor: "#176C8F" },
  {
    key: "prettify-failed",
    label: "Prettify Failed",
    defaultColor: "#B82B2F",
  },
] as const;

export const STATUS_COLOR_SPECS: readonly StatusColorSpec[] = SPECS;

export type StatusColorKey = (typeof SPECS)[number]["key"];

export type StatusColorMap = Record<StatusColorKey, string>;

export const DEFAULT_STATUS_COLORS: StatusColorMap = Object.fromEntries(
  SPECS.map((s) => [s.key, s.defaultColor]),
) as StatusColorMap;

/** The CSS custom property carrying one status's configured color. */
export function statusColorVar(key: StatusColorKey): string {
  return `--status-color-${key}`;
}

const HEX_PATTERN = /^#[0-9A-Fa-f]{6}$/;

/** Only opaque six-digit `#RRGGBB` values are accepted — no shorthand, no
 * alpha channel (WP-88 non-goals). */
export function isValidHexColor(value: string): boolean {
  return HEX_PATTERN.test(value);
}

/** Merge a persisted mapping over the built-in defaults. Anything absent,
 * malformed, unknown, or invalid falls back to the built-in color, so a
 * corrupt or pre-WP-88 store silently yields the default mapping. */
export function parseStatusColors(raw: string | undefined): StatusColorMap {
  const colors = { ...DEFAULT_STATUS_COLORS };
  if (!raw) return colors;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return colors;
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return colors;
  }
  const entries = parsed as Record<string, unknown>;
  for (const spec of SPECS) {
    const value = entries[spec.key];
    if (typeof value === "string" && isValidHexColor(value)) {
      colors[spec.key] = value;
    }
  }
  return colors;
}

/** Serialize the full mapping in spec order, so persisted output is stable. */
export function serializeStatusColors(colors: StatusColorMap): string {
  const ordered: Record<string, string> = {};
  for (const spec of SPECS) ordered[spec.key] = colors[spec.key];
  return JSON.stringify(ordered);
}

/** Push every status color into the document root, switching all visible
 * status surfaces at once. */
export function applyStatusColors(colors: StatusColorMap): void {
  for (const spec of SPECS) {
    document.documentElement.style.setProperty(
      statusColorVar(spec.key),
      colors[spec.key],
    );
  }
}

// WCAG 2.x relative luminance and contrast ratio, for the picker's low-
// contrast warning (advisory only — a low-contrast color may still be saved).
function channelLuminance(channel: number): number {
  const c = channel / 255;
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

function relativeLuminance(hex: string): number {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return (
    0.2126 * channelLuminance(r) +
    0.7152 * channelLuminance(g) +
    0.0722 * channelLuminance(b)
  );
}

export function contrastRatio(hexA: string, hexB: string): number {
  const [lighter, darker] = [
    relativeLuminance(hexA),
    relativeLuminance(hexB),
  ].sort((a, b) => b - a);
  return (lighter + 0.05) / (darker + 0.05);
}

export const WCAG_AA_CONTRAST_RATIO = 4.5;

export function meetsWcagAA(foreground: string, background: string): boolean {
  return contrastRatio(foreground, background) >= WCAG_AA_CONTRAST_RATIO;
}
