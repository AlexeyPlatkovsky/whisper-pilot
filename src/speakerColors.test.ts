import { describe, expect, it } from "vitest";
import { speakerColorClass, speakerLabel } from "./speakerColors";

describe("speakerColorClass", () => {
  it("returns the matching slot for ids within the 10-slot range", () => {
    expect(speakerColorClass(0)).toBe("wp-speaker-color-0");
    expect(speakerColorClass(9)).toBe("wp-speaker-color-9");
  });

  it("wraps around the palette at id 10", () => {
    expect(speakerColorClass(10)).toBe("wp-speaker-color-0");
  });

  it("wraps around correctly for a non-trivial multi-cycle id", () => {
    expect(speakerColorClass(23)).toBe("wp-speaker-color-3");
  });
});

describe("speakerLabel", () => {
  it("formats a 1-indexed human-readable label", () => {
    expect(speakerLabel(0)).toBe("Speaker 1");
    expect(speakerLabel(4)).toBe("Speaker 5");
  });
});
