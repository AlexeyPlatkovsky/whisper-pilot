#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

export function normalizeForAsrScore(text) {
  return text
    .normalize("NFKC")
    .toLocaleLowerCase("und")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim()
    .replace(/\s+/gu, " ");
}

export function editDistance(reference, hypothesis) {
  let previous = Array.from({ length: hypothesis.length + 1 }, (_, index) => index);
  for (let referenceIndex = 0; referenceIndex < reference.length; referenceIndex += 1) {
    const current = [referenceIndex + 1];
    for (let hypothesisIndex = 0; hypothesisIndex < hypothesis.length; hypothesisIndex += 1) {
      const substitution =
        previous[hypothesisIndex] +
        (reference[referenceIndex] === hypothesis[hypothesisIndex] ? 0 : 1);
      current.push(
        Math.min(
          current[hypothesisIndex] + 1,
          previous[hypothesisIndex + 1] + 1,
          substitution,
        ),
      );
    }
    previous = current;
  }
  return previous[hypothesis.length];
}

export function scoreTranscript(reference, hypothesis) {
  const normalizedReference = normalizeForAsrScore(reference);
  const normalizedHypothesis = normalizeForAsrScore(hypothesis);
  const referenceWords = normalizedReference ? normalizedReference.split(" ") : [];
  const hypothesisWords = normalizedHypothesis ? normalizedHypothesis.split(" ") : [];
  const referenceCharacters = [...normalizedReference.replaceAll(" ", "")];
  const hypothesisCharacters = [...normalizedHypothesis.replaceAll(" ", "")];
  const wordErrors = editDistance(referenceWords, hypothesisWords);
  const characterErrors = editDistance(referenceCharacters, hypothesisCharacters);
  return {
    wordErrors,
    referenceWords: referenceWords.length,
    wer: referenceWords.length === 0 ? null : wordErrors / referenceWords.length,
    characterErrors,
    referenceCharacters: referenceCharacters.length,
    cer:
      referenceCharacters.length === 0
        ? null
        : characterErrors / referenceCharacters.length,
  };
}

async function main() {
  const [manifestPath, hypothesisDirectory] = process.argv.slice(2);
  if (!manifestPath || !hypothesisDirectory) {
    throw new Error(
      "usage: score-asr-corpus.mjs <manifest.tsv> <hypothesis-directory>",
    );
  }
  const rows = (await readFile(manifestPath, "utf8"))
    .trim()
    .split(/\r?\n/u)
    .slice(1);
  console.log("file\tWER\tCER\tword_errors\tcharacter_errors");
  for (const row of rows) {
    const [audioFile, , , reference] = row.split("\t");
    if (!audioFile || reference === undefined) {
      throw new Error(`invalid manifest row for ${audioFile || "unknown file"}`);
    }
    const hypothesisPath = path.join(
      hypothesisDirectory,
      `${path.parse(audioFile).name}.txt`,
    );
    const hypothesis = await readFile(hypothesisPath, "utf8");
    const score = scoreTranscript(reference, hypothesis);
    const percentage = (value) =>
      value === null ? "n/a" : `${(value * 100).toFixed(2)}%`;
    console.log(
      [
        audioFile,
        percentage(score.wer),
        percentage(score.cer),
        `${score.wordErrors}/${score.referenceWords}`,
        `${score.characterErrors}/${score.referenceCharacters}`,
      ].join("\t"),
    );
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
