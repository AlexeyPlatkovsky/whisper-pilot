import { describe, expect, it } from "vitest";
import { computeWordDiff } from "./diff";

describe("computeWordDiff", () => {
  // S-14, EP: identical-text partition
  it("marks identical text entirely as same, with no del/add spans", () => {
    const spans = computeWordDiff("hello there friend", "hello there friend");

    expect(spans.every((s) => s.type === "same")).toBe(true);
    expect(spans.map((s) => s.text).join("")).toBe("hello there friend");
  });

  // S-15, EP: pure-deletion partition
  it("marks a removed trailing clause as del, with no add spans", () => {
    const spans = computeWordDiff("hello there my friend", "hello there");

    expect(spans.some((s) => s.type === "add")).toBe(false);
    const removed = spans
      .filter((s) => s.type === "del")
      .map((s) => s.text)
      .join("");
    expect(removed).toContain("my");
    expect(removed).toContain("friend");
  });

  // S-16, EP: pure-addition partition
  it("marks an inserted clause as add, with no del spans", () => {
    const spans = computeWordDiff("hello there", "hello there my friend");

    expect(spans.some((s) => s.type === "del")).toBe(false);
    const added = spans
      .filter((s) => s.type === "add")
      .map((s) => s.text)
      .join("");
    expect(added).toContain("my");
    expect(added).toContain("friend");
  });

  // S-17, EP: mixed replacement partition
  it("marks a replaced phrase as del+add, with unchanged surrounding text as same", () => {
    const spans = computeWordDiff(
      "the quick brown fox jumps",
      "the quick red fox jumps",
    );

    expect(spans.some((s) => s.type === "same" && s.text === "the")).toBe(true);
    expect(spans.some((s) => s.type === "same" && s.text === "fox")).toBe(true);
    expect(spans.some((s) => s.type === "same" && s.text === "jumps")).toBe(
      true,
    );
    expect(spans.some((s) => s.type === "del" && s.text === "brown")).toBe(
      true,
    );
    expect(spans.some((s) => s.type === "add" && s.text === "red")).toBe(true);
  });

  // S-18, BVA: empty-string boundary
  it("marks everything as add when the original is empty", () => {
    const spans = computeWordDiff("", "brand new text");

    expect(spans.every((s) => s.type === "add")).toBe(true);
    expect(spans.map((s) => s.text).join("")).toBe("brand new text");
  });

  it("marks everything as del when the updated text is empty", () => {
    const spans = computeWordDiff("old text gone", "");

    expect(spans.every((s) => s.type === "del")).toBe(true);
    expect(spans.map((s) => s.text).join("")).toBe("old text gone");
  });

  it("returns no spans when both inputs are empty", () => {
    expect(computeWordDiff("", "")).toEqual([]);
  });

  it("preserves whitespace runs verbatim when reconstructing either side", () => {
    const spans = computeWordDiff("a  b", "a  b");
    expect(spans.map((s) => s.text).join("")).toBe("a  b");
  });
});
