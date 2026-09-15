import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const repositoryRoot = process.cwd();
const script = "scripts/check-source-size.mjs";

function command(cwd, executable, args) {
  return spawnSync(executable, args, { cwd, encoding: "utf8" });
}

function lines(count) {
  return Array.from({ length: count }, () => "x").join("\n") + "\n";
}

function createRepository() {
  const root = mkdtempSync(join(tmpdir(), "whisper-pilot-source-size-"));
  mkdirSync(join(root, "scripts"), { recursive: true });
  copyFileSync(join(repositoryRoot, script), join(root, script));
  assert.equal(command(root, "git", ["init", "-q"]).status, 0);
  return root;
}

function stage(root, file, content) {
  const absolute = join(root, file);
  mkdirSync(join(absolute, ".."), { recursive: true });
  writeFileSync(absolute, content);
  assert.equal(command(root, "git", ["add", file]).status, 0);
}

function run(root) {
  return command(root, "node", [script, "--staged"]);
}

test("accepts a production source file at the limit", () => {
  const root = createRepository();
  try {
    stage(root, "src/example.ts", lines(749));
    const result = run(root);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects an oversized staged production source file", () => {
  const root = createRepository();
  try {
    stage(root, "src/example.tsx", lines(750));
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /src\/example\.tsx: 750 lines exceeds the 749-line limit/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects an oversized architecture document", () => {
  const root = createRepository();
  try {
    stage(root, "docs/architecture/example.md", lines(601));
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /docs\/architecture\/example\.md: 601 lines exceeds the 600-line limit/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("keeps the stylesheet entry point small", () => {
  const root = createRepository();
  try {
    stage(root, "src/styles.css", lines(121));
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /src\/styles\.css: 121 lines exceeds the 120-line limit/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("accepts a frontend test file at its limit", () => {
  const root = createRepository();
  try {
    stage(root, "src/example.test.tsx", lines(1_749));
    const result = run(root);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects an oversized frontend test file", () => {
  const root = createRepository();
  try {
    stage(root, "src/example.test.tsx", lines(1_750));
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /src\/example\.test\.tsx: 1750 lines exceeds the 1749-line limit/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("accepts a Rust test module at its limit", () => {
  const root = createRepository();
  try {
    stage(root, "src-tauri/src/decoder/tests.rs", lines(1_749));
    const result = run(root);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects an oversized Rust test module", () => {
  const root = createRepository();
  try {
    stage(root, "src-tauri/src/decoder/tests.rs", lines(1_750));
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /src-tauri\/src\/decoder\/tests\.rs: 1750 lines exceeds the 1749-line limit/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects a production split candidate instead of warning", () => {
  const root = createRepository();
  try {
    stage(root, "src/example.ts", lines(750));
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /src\/example\.ts: 750 lines exceeds the 749-line limit/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("allows Cloud transport only at its fixed deferred baseline", () => {
  const root = createRepository();
  try {
    stage(root, "src-tauri/src/cloud_streaming.rs", lines(935));
    const result = run(root);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stderr, /Deferred size debt: src-tauri\/src\/cloud_streaming\.rs \(935\/935 lines\)/);
    assert.match(result.stderr, /WP-130/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects Cloud transport growth beyond its deferred baseline", () => {
  const root = createRepository();
  try {
    stage(root, "src-tauri/src/cloud_streaming.rs", lines(936));
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(
      result.stderr,
      /src-tauri\/src\/cloud_streaming\.rs: 936 lines exceeds its WP-130 deferred 935-line baseline/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("reads the staged snapshot rather than a later working-tree edit", () => {
  const root = createRepository();
  try {
    stage(root, "src/example.ts", lines(749));
    writeFileSync(join(root, "src/example.ts"), lines(750));
    const result = run(root);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

for (const [name, file] of [
  ["root documentation", "README.md"],
  ["repository scripts", "scripts/example.mjs"],
  ["the Rust build script", "src-tauri/build.rs"],
  ["workflow code", ".github/workflows/example.yml"],
]) {
  test(`covers ${name}`, () => {
    const root = createRepository();
    try {
      stage(root, file, lines(name === "root documentation" ? 601 : 750));
      const result = run(root);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, new RegExp(file.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
