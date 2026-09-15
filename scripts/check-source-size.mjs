#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";

const root = execFileSync("git", ["rev-parse", "--show-toplevel"], {
  encoding: "utf8",
}).trim();
const staged = process.argv.includes("--staged");
// A 750-line production module is already a split candidate, so it fails
// deterministically instead of becoming a warning that review can overlook.
const sourceLimit = 749;
const documentLimit = 600;
const entrypointLimit = 120;
const testLimit = 1_749;
const deferredCloudBaseline = 935;
const deferredCloudFile = "src-tauri/src/cloud_streaming.rs";

function gitLines(args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8" })
    .split("\n")
    .filter(Boolean);
}

function candidates() {
  if (staged) {
    return gitLines([
      "diff",
      "--cached",
      "--name-only",
      "--diff-filter=ACMR",
    ]);
  }
  return gitLines(["ls-files", "--cached", "--others", "--exclude-standard"]);
}

function limitFor(file) {
  if (file === "src/styles.css") return entrypointLimit;
  if ((file.startsWith("docs/") || !file.includes("/")) && file.endsWith(".md")) {
    return documentLimit;
  }
  if (file.startsWith("scripts/") && /\.(?:mjs|js|sh)$/.test(file)) return sourceLimit;
  if (file.startsWith(".githooks/")) return sourceLimit;
  if (file.startsWith(".github/workflows/") && /\.ya?ml$/.test(file)) return sourceLimit;
  if (/\.(?:test|spec)\.(?:ts|tsx)$/.test(file)) return testLimit;
  if (
    file.startsWith("src-tauri/") &&
    (file.startsWith("src-tauri/tests/") || /(?:^|\/)(?:tests?|[^/]+_tests?)\.rs$/.test(file))
  ) {
    return testLimit;
  }
  if (file.startsWith("src/styles/") && file.endsWith(".css")) return sourceLimit;
  if (
    file.startsWith("src/") &&
    /\.(?:ts|tsx|css)$/.test(file) &&
    !/\.(?:test|spec)\.(?:ts|tsx)$/.test(file)
  ) {
    return sourceLimit;
  }
  if ((file.startsWith("src-tauri/src/") || file === "src-tauri/build.rs") && file.endsWith(".rs")) {
    return sourceLimit;
  }
  return undefined;
}

function read(file) {
  if (!staged) return readFileSync(path.join(root, file), "utf8");
  return execFileSync("git", ["show", `:${file}`], {
    cwd: root,
    encoding: "utf8",
  });
}

function countLines(source) {
  if (source.length === 0) return 0;
  return source.split(/\r?\n/).length - (source.endsWith("\n") ? 1 : 0);
}

const failures = [];
const warnings = [];
for (const file of candidates()) {
  const limit = limitFor(file);
  if (!limit) continue;
  const lines = countLines(read(file));
  if (file === deferredCloudFile && lines > deferredCloudBaseline) {
    failures.push(
      `${file}: ${lines} lines exceeds its WP-130 deferred ${deferredCloudBaseline}-line baseline`,
    );
    continue;
  }
  if (file === deferredCloudFile && lines > limit) {
    warnings.push(
      `Deferred size debt: ${file} (${lines}/${deferredCloudBaseline} lines). ` +
        "Cloud transport size debt is tracked in WP-130 and must not grow.",
    );
    continue;
  }
  if (lines > limit) {
    failures.push(`${file}: ${lines} lines exceeds the ${limit}-line limit`);
    continue;
  }
}

for (const warning of warnings) console.warn(warning);

if (failures.length > 0) {
  console.error(["Source-size validation failed:", ...failures.map((item) => `- ${item}`)].join("\n"));
  process.exit(1);
}

console.log(staged ? "Staged source-size validation passed" : "Source-size validation passed");
