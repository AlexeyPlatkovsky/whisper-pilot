import { afterEach, describe, expect, it } from "vitest";
import { applyTheme } from "./theme";

afterEach(() => {
  delete document.documentElement.dataset.theme;
});

describe("applyTheme", () => {
  it("sets data-theme to dark", () => {
    applyTheme("dark");

    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("sets data-theme to light", () => {
    applyTheme("light");

    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("clears data-theme for system, so the OS media query drives it", () => {
    document.documentElement.dataset.theme = "dark";

    applyTheme("system");

    expect(document.documentElement.dataset.theme).toBeUndefined();
  });
});
