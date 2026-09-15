import assert from "node:assert/strict";
import test from "node:test";

import { editDistance, normalizeForAsrScore, scoreTranscript } from "./score-asr-corpus.mjs";

test("ASR normalization is Unicode-aware and removes punctuation", () => {
  assert.equal(normalizeForAsrScore("  Привет, WORLD!  "), "привет world");
});

test("edit distance counts insertions, deletions, and substitutions", () => {
  assert.equal(editDistance(["a", "b", "c"], ["a", "x", "c", "d"]), 2);
});

test("WER and CER expose exact denominators without transcript content", () => {
  assert.deepEqual(scoreTranscript("one two", "one too"), {
    wordErrors: 1,
    referenceWords: 2,
    wer: 0.5,
    characterErrors: 1,
    referenceCharacters: 6,
    cer: 1 / 6,
  });
});
