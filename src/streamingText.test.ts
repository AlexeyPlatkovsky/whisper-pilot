import { describe, expect, it, vi } from "vitest";
import type { StreamingWindow } from "./ipc";
import { upsertWindow } from "./streamingText";

function windowAt(index: number, text = `window ${index}`): StreamingWindow {
  return {
    window_index: index,
    start_ms: index * 7_000,
    end_ms: (index + 1) * 7_000,
    text,
    language: "en",
    outcome_ok: true,
  };
}

describe("upsertWindow", () => {
  it("appends the next monotonic live window without globally reordering prior windows", () => {
    const current = [windowAt(0), windowAt(1), windowAt(2)];
    const sort = vi.spyOn(Array.prototype, "sort");

    try {
      const next = upsertWindow(current, windowAt(3));

      expect(next.map((window) => window.window_index)).toEqual([0, 1, 2, 3]);
      expect(sort).not.toHaveBeenCalled();
    } finally {
      sort.mockRestore();
    }
  });

  it("replaces a resent index while preserving ascending transcript order", () => {
    const next = upsertWindow(
      [windowAt(0), windowAt(1), windowAt(2)],
      windowAt(1, "corrected middle window"),
    );

    expect(next.map((window) => window.window_index)).toEqual([0, 1, 2]);
    expect(next[1]?.text).toBe("corrected middle window");
  });
});
