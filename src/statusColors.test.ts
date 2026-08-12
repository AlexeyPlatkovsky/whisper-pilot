import { afterEach, describe, expect, it } from "vitest";
import {
  applyStatusColors,
  contrastRatio,
  DEFAULT_STATUS_COLORS,
  isValidHexColor,
  meetsWcagAA,
  parseStatusColors,
  serializeStatusColors,
  STATUS_COLOR_SPECS,
  statusColorVar,
} from "./statusColors";

afterEach(() => {
  for (const spec of STATUS_COLOR_SPECS) {
    document.documentElement.style.removeProperty(statusColorVar(spec.key));
  }
});

describe("STATUS_COLOR_SPECS", () => {
  it("lists each current semantic status exactly once with its built-in default", () => {
    expect(STATUS_COLOR_SPECS).toEqual([
      { key: "no-files", label: "No files", defaultColor: "#8A5F10" },
      { key: "ready", label: "Ready", defaultColor: "#5A7684" },
      { key: "transcribing", label: "Transcribing", defaultColor: "#176C8F" },
      { key: "diarizing", label: "Diarizing", defaultColor: "#176C8F" },
      {
        key: "crafting-notes",
        label: "Crafting notes",
        defaultColor: "#176C8F",
      },
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
    ]);
  });
});

describe("isValidHexColor", () => {
  // EP: the valid partition — opaque six-digit hex only.
  it("accepts opaque six-digit hex colors", () => {
    expect(isValidHexColor("#8A5F10")).toBe(true);
    expect(isValidHexColor("#abcdef")).toBe(true);
    expect(isValidHexColor("#000000")).toBe(true);
  });

  // EP: invalid partitions — shorthand, alpha-bearing, missing #, non-hex,
  // empty.
  it("rejects shorthand, alpha-bearing, and malformed values", () => {
    expect(isValidHexColor("#123")).toBe(false);
    expect(isValidHexColor("#12345678")).toBe(false);
    expect(isValidHexColor("123456")).toBe(false);
    expect(isValidHexColor("#GGGGGG")).toBe(false);
    expect(isValidHexColor("")).toBe(false);
    expect(isValidHexColor("#12345")).toBe(false);
  });
});

describe("parseStatusColors", () => {
  it("returns the built-in mapping when nothing is saved", () => {
    expect(parseStatusColors(undefined)).toEqual(DEFAULT_STATUS_COLORS);
  });

  it("returns the built-in mapping for malformed JSON", () => {
    expect(parseStatusColors("not json")).toEqual(DEFAULT_STATUS_COLORS);
    expect(parseStatusColors("[]")).toEqual(DEFAULT_STATUS_COLORS);
    expect(parseStatusColors('"#8A5F10"')).toEqual(DEFAULT_STATUS_COLORS);
  });

  it("merges a saved partial mapping over the built-in defaults", () => {
    const parsed = parseStatusColors(
      JSON.stringify({ ready: "#112233", "on-air": "#A0B0C0" }),
    );

    expect(parsed.ready).toBe("#112233");
    expect(parsed["on-air"]).toBe("#A0B0C0");
    expect(parsed.error).toBe(DEFAULT_STATUS_COLORS.error);
  });

  it("ignores unknown keys and invalid saved values", () => {
    const parsed = parseStatusColors(
      JSON.stringify({
        ready: "#123",
        "crafting-mfu": "#ZZZZZZ",
        transmogrifying: "#112233",
      }),
    );

    expect(parsed.ready).toBe(DEFAULT_STATUS_COLORS.ready);
    expect(parsed["crafting-mfu"]).toBe(DEFAULT_STATUS_COLORS["crafting-mfu"]);
    expect("transmogrifying" in parsed).toBe(false);
  });

  it("round-trips through serializeStatusColors", () => {
    const custom = { ...DEFAULT_STATUS_COLORS, error: "#010203" };

    expect(parseStatusColors(serializeStatusColors(custom))).toEqual(custom);
  });
});

describe("applyStatusColors", () => {
  it("writes one CSS custom property per status to the document root", () => {
    const colors = { ...DEFAULT_STATUS_COLORS, ready: "#112233" };

    applyStatusColors(colors);

    const style = document.documentElement.style;
    expect(style.getPropertyValue("--status-color-ready")).toBe("#112233");
    expect(style.getPropertyValue("--status-color-error")).toBe("#B82B2F");
    expect(style.getPropertyValue("--status-color-prettify-failed")).toBe(
      "#B82B2F",
    );
  });
});

describe("contrastRatio", () => {
  it("is 1 for identical colors and 21 for black on white", () => {
    expect(contrastRatio("#FFFFFF", "#FFFFFF")).toBeCloseTo(1);
    expect(contrastRatio("#000000", "#FFFFFF")).toBeCloseTo(21, 0);
  });

  it("is symmetric", () => {
    expect(contrastRatio("#8A5F10", "#FFFFFF")).toBeCloseTo(
      contrastRatio("#FFFFFF", "#8A5F10"),
    );
  });
});

describe("meetsWcagAA", () => {
  it("requires at least 4.5:1", () => {
    expect(meetsWcagAA("#000000", "#FFFFFF")).toBe(true);
    // #767676 on white is the canonical 4.54:1 boundary pass.
    expect(meetsWcagAA("#767676", "#FFFFFF")).toBe(true);
    expect(meetsWcagAA("#CCCCCC", "#FFFFFF")).toBe(false);
  });
});
