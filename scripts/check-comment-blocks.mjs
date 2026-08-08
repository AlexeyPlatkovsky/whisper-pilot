#!/usr/bin/env node
// Flags contiguous comment blocks longer than MAX_COMMENT_LINES. Checks size
// only, never content — a long block is flagged whether or not it "reads"
// like documentation, to keep the check false-positive-free.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { extname, join } from "node:path";

const MAX_COMMENT_LINES = Number(process.env.MAX_COMMENT_LINES ?? 8);

const ROOTS = ["src", "src-tauri/src"];
const EXTENSIONS = new Set([".ts", ".tsx", ".rs"]);
const SKIP_DIR_NAMES = new Set(["node_modules", "target", "dist", "coverage"]);
const SKIP_SUFFIXES = [".test.ts", ".test.tsx"];

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    if (SKIP_DIR_NAMES.has(entry)) continue;
    const full = join(dir, entry);
    const info = statSync(full);
    if (info.isDirectory()) {
      walk(full, out);
    } else if (EXTENSIONS.has(extname(entry))) {
      if (SKIP_SUFFIXES.some((suffix) => entry.endsWith(suffix))) continue;
      out.push(full);
    }
  }
  return out;
}

// A line is comment-only when, after trimming, it consists solely of a
// line-comment (//, ///, //!) or is inside/opens/closes a block comment
// (/* ... */). Code sharing a line with a comment (trailing comments) does
// not count — only lines that are comment for their entire content.
function findViolations(path, text) {
  const lines = text.split("\n");
  const violations = [];
  let blockStart = null;
  let blockLen = 0;
  let inBlockComment = false;

  function flush(endLineIdx) {
    if (blockStart !== null && blockLen > MAX_COMMENT_LINES) {
      violations.push({ path, startLine: blockStart + 1, length: blockLen });
    }
    blockStart = null;
    blockLen = 0;
  }

  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const trimmed = raw.trim();

    if (inBlockComment) {
      if (blockStart === null) blockStart = i;
      blockLen++;
      if (trimmed.includes("*/")) inBlockComment = false;
      continue;
    }

    const isLineComment = /^(\/\/\/?|\/\/!)/.test(trimmed);
    const opensBlockComment = trimmed.startsWith("/*");

    if (isLineComment) {
      if (blockStart === null) blockStart = i;
      blockLen++;
      continue;
    }

    if (opensBlockComment) {
      if (blockStart === null) blockStart = i;
      blockLen++;
      if (!trimmed.includes("*/")) inBlockComment = true;
      continue;
    }

    flush(i);
  }
  flush(lines.length);

  return violations;
}

const files = ROOTS.flatMap((root) => walk(root));
const allViolations = files.flatMap((path) =>
  findViolations(path, readFileSync(path, "utf8")),
);

if (allViolations.length > 0) {
  console.error(
    `Found ${allViolations.length} comment block(s) longer than ${MAX_COMMENT_LINES} lines:\n`,
  );
  for (const v of allViolations) {
    console.error(`  ${v.path}:${v.startLine} — ${v.length} lines`);
  }
  console.error(
    "\nCode files should stay code, not app docs — trim the comment or move the content to docs/.",
  );
  process.exit(1);
}

console.log(
  `No comment blocks longer than ${MAX_COMMENT_LINES} lines across ${files.length} files.`,
);
