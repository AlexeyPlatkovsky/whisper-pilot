// Settings → Appearance → Status Colors (WP-88): one row per current semantic
// status in two columns, each opening an anchored picker popover; a revert
// icon on non-default rows; Reset all behind a confirmation modal. Edits
// preview only inside the popover — Save applies the color to every status
// surface at once and persists it; a persist failure restores the prior
// mapping everywhere.

import { useEffect, useRef, useState } from "react";
import { Icon } from "./Icon";
import { setSetting } from "./ipc";
import {
  applyStatusColors,
  contrastRatio,
  DEFAULT_STATUS_COLORS,
  isValidHexColor,
  parseStatusColors,
  serializeStatusColors,
  STATUS_COLOR_SPECS,
  WCAG_AA_CONTRAST_RATIO,
  type StatusColorKey,
  type StatusColorMap,
} from "./statusColors";

const ALPHABETICAL_STATUS_COLOR_SPECS = [...STATUS_COLOR_SPECS].sort((a, b) =>
  a.label.localeCompare(b.label),
);

// Assign alternating alphabetical entries to each column. In the two-column
// layout, this fills each visual row from left to right and adapts when the
// status catalog changes without requiring an updated split point.
const LEFT_COLUMN = ALPHABETICAL_STATUS_COLOR_SPECS.filter(
  (_, index) => index % 2 === 0,
);
const RIGHT_COLUMN = ALPHABETICAL_STATUS_COLOR_SPECS.filter(
  (_, index) => index % 2 === 1,
);

/** The picker uses a practical two-decimal threshold: 4.495 rounds half-up to
 * 4.50 and passes, while 4.4949 rounds to 4.49 and warns. */
function roundContrastRatio(contrast: number): number {
  return Math.round((contrast + Number.EPSILON) * 100) / 100;
}

/** The background the status widgets sit on, for the contrast warning.
 * Falls back to the light panel when no theme value is readable (tests). */
function statusWidgetBackground(): string {
  const panel = getComputedStyle(document.documentElement)
    .getPropertyValue("--panel")
    .trim();
  return isValidHexColor(panel) ? panel : "#FFFFFF";
}

export function StatusColorsSection({
  statusColorsRaw,
}: {
  statusColorsRaw?: string;
}) {
  const [colors, setColors] = useState<StatusColorMap>(() =>
    parseStatusColors(statusColorsRaw),
  );
  const [openKey, setOpenKey] = useState<StatusColorKey | null>(null);
  const [draft, setDraft] = useState("");
  const [draftError, setDraftError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);
  // Latest-action-wins guard: a stale persist response must not revert a
  // newer user action's mapping (WP-88 DoD).
  const requestRef = useRef(0);

  async function persist(next: StatusColorMap) {
    const requestId = ++requestRef.current;
    const previous = colors;
    setActionError(null);
    setColors(next);
    applyStatusColors(next);
    try {
      await setSetting("status_colors", serializeStatusColors(next));
    } catch (e) {
      if (requestId !== requestRef.current) return;
      setColors(previous);
      applyStatusColors(previous);
      setActionError(String(e));
    }
  }

  function openPicker(key: StatusColorKey) {
    setOpenKey(key);
    setDraft(colors[key]);
    setDraftError(null);
  }

  function closePicker() {
    setOpenKey(null);
    setDraftError(null);
  }

  function savePicker() {
    if (!openKey) return;
    if (!isValidHexColor(draft)) {
      setDraftError("Enter an opaque six-digit hex color (#RRGGBB).");
      return;
    }
    // Normalize case so re-typing the built-in hex in lowercase is the
    // default color again, not a "custom" one with a revert icon.
    const next = { ...colors, [openKey]: draft.toUpperCase() };
    closePicker();
    void persist(next);
  }

  // Escape closes the popover / confirmation without changing anything.
  useEffect(() => {
    if (!openKey && !confirmReset) return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key !== "Escape") return;
      closePicker();
      setConfirmReset(false);
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [openKey, confirmReset]);

  const contrast = isValidHexColor(draft)
    ? contrastRatio(draft, statusWidgetBackground())
    : null;
  const roundedContrast =
    contrast === null ? null : roundContrastRatio(contrast);
  const lowContrast =
    roundedContrast !== null && roundedContrast < WCAG_AA_CONTRAST_RATIO;

  function renderRow(spec: (typeof STATUS_COLOR_SPECS)[number]) {
    const color = colors[spec.key];
    const isDefault = color === spec.defaultColor;
    return (
      <div className="wp-status-color-row" key={spec.key}>
        <button
          type="button"
          className="wp-status-color-pick"
          onClick={() => openPicker(spec.key)}
        >
          <span className="wp-status-color-name" style={{ color }}>
            {spec.label}
          </span>
          <span className="wp-status-color-spacer" />
          <span
            className="wp-status-color-swatch"
            style={{ backgroundColor: color }}
          />
          <span className="wp-status-color-hex">{color}</span>
        </button>
        {/* The slot is always rendered at its fixed size so the row's
            dimensions never change when the revert icon appears. */}
        <span className="wp-status-color-revert-slot">
          {!isDefault && (
            <button
              type="button"
              className="wp-status-color-revert"
              aria-label={`Revert ${spec.label} to default color`}
              title={`Revert ${spec.label} to ${spec.defaultColor}`}
              onClick={() =>
                void persist({ ...colors, [spec.key]: spec.defaultColor })
              }
            >
              <Icon name="rotate-ccw" size={13} />
            </button>
          )}
        </span>
        {openKey === spec.key && (
          <div
            className="wp-status-color-popover"
            role="dialog"
            aria-label={`${spec.label} color picker`}
          >
            <div className="wp-status-color-popover-inputs">
              <input
                type="color"
                aria-label="Visual color picker"
                value={isValidHexColor(draft) ? draft : "#000000"}
                onChange={(e) => {
                  setDraft(e.target.value.toUpperCase());
                  setDraftError(null);
                }}
              />
              <input
                type="text"
                aria-label="Hex color"
                value={draft}
                autoFocus
                onChange={(e) => {
                  setDraft(e.target.value);
                  setDraftError(null);
                }}
              />
            </div>
            {lowContrast && (
              <p className="wp-status-color-warning">
                Low contrast ({roundedContrast.toFixed(2)}&lt;
                {WCAG_AA_CONTRAST_RATIO.toFixed(2)}:1) against the current theme
                background.
              </p>
            )}
            {draftError && <p role="alert">{draftError}</p>}
            <div className="wp-status-color-popover-actions">
              <button type="button" onClick={closePicker}>
                Cancel
              </button>
              <button type="button" onClick={savePicker}>
                Save
              </button>
            </div>
          </div>
        )}
      </div>
    );
  }

  return (
    <section className="wp-status-colors" aria-label="Status Colors">
      <div className="wp-status-colors-header">
        <div>
          <h3 className="wp-status-colors-title">Status Colors</h3>
          <p className="wp-status-colors-description">
            Colors used for pipeline status labels across the app.
          </p>
        </div>
        <button
          type="button"
          className="wp-status-colors-reset"
          onClick={() => setConfirmReset(true)}
        >
          <Icon name="rotate-ccw" size={14} />
          Reset all colors
        </button>
      </div>
      <div className="wp-status-color-grid">
        <div className="wp-status-color-column">
          {LEFT_COLUMN.map(renderRow)}
        </div>
        <div className="wp-status-color-column">
          {RIGHT_COLUMN.map(renderRow)}
        </div>
      </div>
      {actionError && <p role="alert">{actionError}</p>}
      {confirmReset && (
        <div className="modal-overlay">
          <div
            className="modal-panel confirm-modal"
            role="alertdialog"
            aria-modal="true"
            aria-label="Reset all colors"
          >
            <div className="modal-header">
              <span className="modal-title">Reset all colors?</span>
            </div>
            <p className="confirm-warning">
              This restores the built-in color for every status.
            </p>
            <div className="confirm-actions">
              <button type="button" onClick={() => setConfirmReset(false)}>
                Cancel
              </button>
              <button
                type="button"
                onClick={() => {
                  setConfirmReset(false);
                  void persist({ ...DEFAULT_STATUS_COLORS });
                }}
              >
                Reset all
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
