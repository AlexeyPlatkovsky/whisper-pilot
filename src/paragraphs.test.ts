import { describe, expect, it } from "vitest";
import { groupWindowsIntoParagraphs } from "./paragraphs";

function ok(text: string) {
  return { text, outcome_ok: true };
}

function failed() {
  return { text: "", outcome_ok: false };
}

describe("groupWindowsIntoParagraphs", () => {
  // BVA: the empty-list lower boundary
  it("returns no paragraphs for an empty window list", () => {
    expect(groupWindowsIntoParagraphs([])).toEqual([]);
  });

  // EP: short, sentence-ending windows stay in one paragraph below the
  // length threshold
  it("keeps short windows in a single paragraph", () => {
    const windows = [ok("Hello there."), ok("How are you?")];

    const paragraphs = groupWindowsIntoParagraphs(windows);

    expect(paragraphs).toEqual([windows]);
  });

  // S-1: happy path — a paragraph closes once it's both long enough and
  // ends a sentence, and a fresh one starts after it
  it("starts a new paragraph once a long-enough sentence ends", () => {
    const long = "x".repeat(240) + ".";
    const windows = [ok(long), ok("Next paragraph starts here.")];

    const paragraphs = groupWindowsIntoParagraphs(windows);

    expect(paragraphs).toEqual([[windows[0]], [windows[1]]]);
  });

  // EP: reaching the length threshold mid-sentence (no terminal punctuation)
  // must not split — only a sentence boundary can close a paragraph on
  // length alone
  it("does not split on length alone without a sentence boundary", () => {
    const noPunctuation = "x".repeat(400);
    const windows = [ok(noPunctuation), ok("still going")];

    const paragraphs = groupWindowsIntoParagraphs(windows);

    expect(paragraphs).toEqual([windows]);
  });

  // Regression: acoustic windows are not sentence boundaries. Even a long
  // run of windows must stay together until terminal punctuation arrives.
  it("does not split five-plus windows in the middle of a sentence", () => {
    const windows = Array.from({ length: 7 }, (_, i) =>
      ok(`unfinished clause ${i} keeps going`),
    );

    const paragraphs = groupWindowsIntoParagraphs(windows);

    expect(paragraphs).toEqual([windows]);
  });

  it("uses a large hard fallback when punctuation never arrives", () => {
    const windows = Array.from({ length: 13 }, (_, i) =>
      ok(`unpunctuated streaming window ${i} keeps growing`),
    );

    expect(groupWindowsIntoParagraphs(windows)).toEqual([
      windows.slice(0, 12),
      windows.slice(12),
    ]);
  });

  it("closes a soft-length paragraph at the next sentence ending", () => {
    const windows = [
      ...Array.from({ length: 5 }, (_, i) =>
        ok(`unfinished clause ${i} ${"x".repeat(45)}`),
      ),
      ok("The sentence finally ends here."),
      ok("A new paragraph starts here."),
    ];

    const paragraphs = groupWindowsIntoParagraphs(windows);

    expect(paragraphs).toEqual([windows.slice(0, 6), windows.slice(6)]);
  });

  it("recognizes an ellipsis and closing quotation mark as a sentence ending", () => {
    const windows = [
      ...Array.from({ length: 4 }, (_, i) => ok(`quoted clause ${i}`)),
      ok("Так он и сказал…»"),
      ok("Следующая мысль"),
    ];

    expect(groupWindowsIntoParagraphs(windows)).toEqual([
      windows.slice(0, 5),
      windows.slice(5),
    ]);
  });

  // EP: a fail-open window's empty text can't itself end a sentence
  it("a fail-open window does not count as ending a sentence", () => {
    const long = "x".repeat(240) + ".";
    const windows = [ok(long), failed()];

    const paragraphs = groupWindowsIntoParagraphs(windows);

    expect(paragraphs).toEqual([[windows[0]], [windows[1]]]);
  });

  // Decision-table: quote/paren after terminal punctuation still counts as
  // a sentence end
  it("recognizes a sentence end followed by a closing quote or paren", () => {
    const long = "x".repeat(240) + '."';
    const windows = [ok(long), ok("Next one.")];

    const paragraphs = groupWindowsIntoParagraphs(windows);

    expect(paragraphs).toEqual([[windows[0]], [windows[1]]]);
  });
});
