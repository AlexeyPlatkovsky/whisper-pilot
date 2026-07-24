import { describe, expect, it } from "vitest";
import { t } from "./i18n";

describe("t", () => {
  it("looks up an English string by key", () => {
    expect(t("addFile")).toBe("Add file");
    expect(t("save")).toBe("Save");
  });

  it("defaults to the English locale when none is given", () => {
    expect(t("emptyState")).toBe(
      "Add an audio or video file to get a transcript.",
    );
  });

  it("returns the exact model-missing message used by the availability warning", () => {
    expect(t("modelMissing")).toBe(
      "The Whisper model isn't downloaded. Open Settings → AI models to download it.",
    );
  });
});
