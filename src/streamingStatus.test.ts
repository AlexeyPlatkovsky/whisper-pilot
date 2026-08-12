import { describe, expect, it } from "vitest";
import {
  resolveStreamingRowStatus,
  resolveStreamingWidgetStatus,
} from "./streamingStatus";

// WP-88: pins which configurable status color each streaming state resolves
// to, so the shared statuses (Ready, Finished, Unknown) can't silently drift
// away from Meeting's mapping.
describe("resolveStreamingWidgetStatus — statusKey mapping", () => {
  it.each([
    // Ready shares Meeting's "ready" color, even though its tone class is
    // "finished" — a configured Ready color must reach both surfaces.
    ["ready", "ready"],
    ["starting", "starting"],
    ["on-air", "on-air"],
    ["crafting", "crafting-mfu"],
    ["mfu-failed", "mfu-failed"],
    ["prettifying", "prettifying"],
    ["prettify-failed", "prettify-failed"],
  ] as const)("%s resolves to statusKey %s", (state, statusKey) => {
    expect(resolveStreamingWidgetStatus(state).statusKey).toBe(statusKey);
  });
});

describe("resolveStreamingRowStatus — statusKey mapping", () => {
  it("maps a stopped session row to the shared Finished status", () => {
    expect(resolveStreamingRowStatus("stopped").statusKey).toBe("finished");
  });

  it("maps an abandoned active session row to the shared Unknown status", () => {
    expect(resolveStreamingRowStatus("active").statusKey).toBe("unknown");
  });
});
