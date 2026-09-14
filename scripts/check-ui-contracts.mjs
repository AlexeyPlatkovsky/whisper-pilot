#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const root = execFileSync("git", ["rev-parse", "--show-toplevel"], {
  encoding: "utf8",
}).trim();

function walk(relative) {
  const absolute = path.join(root, relative);
  if (!existsSync(absolute)) return [];
  if (!statSync(absolute).isDirectory()) return [relative];
  return readdirSync(absolute).flatMap((entry) =>
    walk(path.join(relative, entry)),
  );
}

const errors = [];
for (const file of walk("src").filter((entry) => entry.endsWith(".tsx"))) {
  if (file === "src/ConfirmDialog.tsx") continue;
  const source = readFileSync(path.join(root, file), "utf8");
  if (/role\s*=\s*["']alertdialog["']/.test(source)) {
    errors.push(`${file}: confirmation dialogs must use src/ConfirmDialog.tsx`);
  }
  for (const call of source.matchAll(/<ConfirmDialog\b[\s\S]*?\/>/g)) {
    const markup = call[0];
    const explicitTone =
      /\bdestructive(?:\s*=\s*\{(?:true|false)\}|(?=\s|\/>))/.test(markup);
    if (!explicitTone) {
      errors.push(
        `${file}: ConfirmDialog must select an explicit destructive tone`,
      );
    }
    const label = markup.match(/confirmLabel\s*=\s*["']([^"']+)["']/)?.[1];
    if (
      label &&
      /\b(delete|clear|remove)\b/i.test(label) &&
      /\bdestructive\s*=\s*\{false\}/.test(markup)
    ) {
      errors.push(
        `${file}: ${label} confirmation must use the destructive tone`,
      );
    }
  }
}

const dialog = readFileSync(path.join(root, "src/ConfirmDialog.tsx"), "utf8");
if (!dialog.includes("modal-button--danger")) {
  errors.push("src/ConfirmDialog.tsx: destructive action variant is missing");
}
const styles = readFileSync(path.join(root, "src/styles.css"), "utf8");
if (!/\.modal-button--danger\s*\{[^}]*var\(--error\)/s.test(styles)) {
  errors.push("src/styles.css: destructive modal action must use var(--error)");
}

if (errors.length > 0) {
  console.error(["UI contract validation failed:", ...errors].join("\n- "));
  process.exit(1);
}

console.log("UI contract validation passed");
