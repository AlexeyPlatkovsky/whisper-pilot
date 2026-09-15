import { describe, expect, it } from "vitest";
import {
  isLiveCaptureActive,
  reconcileLiveCaptureSnapshot,
  type LiveCapturePhase,
  type LiveCaptureSnapshot,
} from "./liveCaptureState";

function snapshot(
  phase: LiveCapturePhase,
  revision: number,
  overrides: Partial<LiveCaptureSnapshot> = {},
): LiveCaptureSnapshot {
  return {
    phase,
    session_id: phase === "idle" ? null : 41,
    generation: phase === "idle" ? 0 : 3,
    revision,
    error: null,
    ...overrides,
  };
}

// WP-112: a newly mounted renderer hydrates from the Rust snapshot rather
// than assuming capture is idle from component-local initial state.
describe("reconcileLiveCaptureSnapshot", () => {
  it("hydrates a remounted client from the current backend capture", () => {
    const backend = snapshot("capturing", 12);

    const hydrated = reconcileLiveCaptureSnapshot(null, backend);

    expect(hydrated).toEqual(backend);
    expect(isLiveCaptureActive(hydrated)).toBe(true);
    expect(hydrated.session_id).toBe(41);
    expect(hydrated.generation).toBe(3);
  });

  it("accepts only newer revisions and ignores delayed or duplicate events", () => {
    const current = snapshot("capturing", 12);
    const delayedPriorGeneration = snapshot("stopping", 11, {
      session_id: 9,
      generation: 2,
    });
    const conflictingDuplicate = snapshot("error", 12, {
      error: "late error",
    });
    const next = snapshot("stopping", 13);

    expect(reconcileLiveCaptureSnapshot(current, delayedPriorGeneration)).toBe(
      current,
    );
    expect(reconcileLiveCaptureSnapshot(current, conflictingDuplicate)).toBe(
      current,
    );
    expect(reconcileLiveCaptureSnapshot(current, next)).toBe(next);
  });
});

describe("isLiveCaptureActive", () => {
  it.each([
    ["idle", false],
    ["starting", true],
    ["capturing", true],
    ["stopping", true],
    ["error", false],
  ] as const)(
    "derives %s exclusively from the backend phase",
    (phase, active) => {
      expect(isLiveCaptureActive(snapshot(phase, 1))).toBe(active);
    },
  );
});
