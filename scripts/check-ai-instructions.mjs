#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";

const root = execFileSync("git", ["rev-parse", "--show-toplevel"], {
  encoding: "utf8",
}).trim();
const staged = process.argv.includes("--staged");
const maxLines = 100;
const maxWidth = 120;

function active(file) {
  const value = file.split(path.sep).join("/");
  return (
    value === "AGENTS.md" ||
    /^\.agents\/skills\/.+\.md$/.test(value) ||
    /^\.codex\/agents\/[^/]+\.toml$/.test(value) ||
    /^\.claude\/conventions\/.+\.md$/.test(value) ||
    /^\.claude\/sdd\/templates\/.+\.md$/.test(value)
  );
}

function walk(relative) {
  const absolute = path.join(root, relative);
  if (!existsSync(absolute)) return [];
  if (!statSync(absolute).isDirectory()) return [relative];
  return readdirSync(absolute).flatMap((entry) =>
    walk(path.join(relative, entry)),
  );
}

function stagedFiles() {
  const output = execFileSync(
    "git",
    ["diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"],
    { cwd: root },
  );
  return output.toString("utf8").split("\0").filter(Boolean).filter(active);
}

function allFiles() {
  return [
    "AGENTS.md",
    ...walk(".agents/skills"),
    ...walk(".codex/agents"),
    ...walk(".claude/conventions"),
    ...walk(".claude/sdd/templates"),
  ].filter(active);
}

function content(file) {
  if (!staged) return readFileSync(path.join(root, file), "utf8");
  return execFileSync("git", ["show", ":" + file], {
    cwd: root,
    encoding: "utf8",
  });
}

const errors = [];
for (const file of staged ? stagedFiles() : allFiles()) {
  const text = content(file);
  const normalized = text.endsWith("\n") ? text.slice(0, -1) : text;
  const lines = normalized ? normalized.split(/\r?\n/) : [];

  if (lines.length > maxLines) {
    errors.push(file + ": " + lines.length + " lines; maximum is " + maxLines);
  }
  lines.forEach((line, index) => {
    const width = Array.from(line).length;
    if (width > maxWidth) {
      errors.push(
        file +
          ":" +
          (index + 1) +
          ": " +
          width +
          " characters; maximum is " +
          maxWidth,
      );
    }
  });

  if (file.endsWith("/SKILL.md")) {
    const frontmatter = text.match(/^---\r?\n([\s\S]*?)\r?\n---/);
    const hasName =
      frontmatter && /^name: [a-z][a-z0-9-]*$/m.test(frontmatter[1]);
    const hasDescription =
      frontmatter && /^description: \S.+$/m.test(frontmatter[1]);
    if (!hasName || !hasDescription) {
      errors.push(file + ": frontmatter must contain name and description");
    }
  }
  if (file.startsWith(".codex/agents/")) {
    for (const field of ["name", "description", "developer_instructions"]) {
      if (!new RegExp("^" + field + "\\s*=", "m").test(text)) {
        errors.push(file + ": missing " + field);
      }
    }
    if (!text.includes("Isolation reason:")) {
      errors.push(file + ": missing explicit isolation reason");
    }
  }
}

if (errors.length) {
  console.error(
    [
      "AI instruction validation failed:",
      ...errors.map((error) => "- " + error),
    ].join("\n"),
  );
  process.exit(1);
}

console.log(
  "AI instruction validation passed (" +
    (staged ? "staged files" : "active landscape") +
    ")",
);
