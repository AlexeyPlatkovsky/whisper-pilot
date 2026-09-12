import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const repositoryRoot = process.cwd();

function command(cwd, executable, args) {
  return spawnSync(executable, args, { cwd, encoding: "utf8" });
}

function createRepository() {
  const root = mkdtempSync(join(tmpdir(), "whisper-pilot-pre-commit-"));
  mkdirSync(join(root, ".githooks"), { recursive: true });
  mkdirSync(join(root, "scripts"), { recursive: true });
  mkdirSync(join(root, "src-tauri"), { recursive: true });
  copyFileSync(
    join(repositoryRoot, ".githooks/pre-commit"),
    join(root, ".githooks/pre-commit"),
  );
  copyFileSync(
    join(repositoryRoot, "scripts/bump-version.sh"),
    join(root, "scripts/bump-version.sh"),
  );
  chmodSync(join(root, ".githooks/pre-commit"), 0o755);
  chmodSync(join(root, "scripts/bump-version.sh"), 0o755);
  writeFileSync(join(root, "scripts/check-ai-instructions.mjs"), "");
  writeFileSync(
    join(root, "src-tauri/Cargo.toml"),
    '[package]\nversion = "1.10.0"\n',
  );
  writeFileSync(
    join(root, "src-tauri/tauri.conf.json"),
    '{\n  "version": "1.10.0"\n}\n',
  );
  writeFileSync(join(root, "package.json"), '{"version":"1.10.0"}\n');
  writeFileSync(
    join(root, "package-lock.json"),
    '{"version":"1.10.0","packages":{"":{"version":"1.10.0"}}}\n',
  );
  writeFileSync(join(root, "product.txt"), "baseline\n");

  assert.equal(command(root, "git", ["init", "-q"]).status, 0);
  assert.equal(command(root, "git", ["config", "user.name", "Test"]).status, 0);
  assert.equal(
    command(root, "git", ["config", "user.email", "test@example.com"]).status,
    0,
  );
  assert.equal(command(root, "git", ["add", "."]).status, 0);
  assert.equal(
    command(root, "git", [
      "-c",
      "core.hooksPath=/dev/null",
      "commit",
      "-qm",
      "baseline",
    ]).status,
    0,
  );
  return root;
}

function runHook(root) {
  return command(root, "bash", [join(root, ".githooks/pre-commit")]);
}

function writeVersion(root, version) {
  writeFileSync(
    join(root, "src-tauri/Cargo.toml"),
    `[package]\nversion = "${version}"\n`,
  );
  writeFileSync(
    join(root, "src-tauri/tauri.conf.json"),
    `{\n  "version": "${version}"\n}\n`,
  );
  writeFileSync(
    join(root, "package.json"),
    `{\n  "version": "${version}"\n}\n`,
  );
  writeFileSync(
    join(root, "package-lock.json"),
    `{\n  "version": "${version}",\n  "packages": {\n    "": {\n      "version": "${version}"\n    }\n  }\n}\n`,
  );
}

test("allows a product commit without changing the release version", () => {
  const root = createRepository();
  try {
    writeFileSync(join(root, "product.txt"), "ordinary product change\n");
    assert.equal(command(root, "git", ["add", "product.txt"]).status, 0);

    const result = runHook(root);

    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("allows non-version manifest changes without a release bump", () => {
  const root = createRepository();
  try {
    writeFileSync(
      join(root, "src-tauri/Cargo.toml"),
      '[package]\nversion = "1.10.0"\n\n[dependencies]\nserde = "1"\n',
    );
    assert.equal(
      command(root, "git", ["add", "src-tauri/Cargo.toml"]).status,
      0,
    );

    const result = runHook(root);

    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects a partially staged release-version change", () => {
  const root = createRepository();
  try {
    writeVersion(root, "1.10.1");
    assert.equal(command(root, "git", ["add", "package.json"]).status, 0);

    const result = runHook(root);

    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /staged version sources disagree/i);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("allows a synchronized release-version change when every source is staged", () => {
  const root = createRepository();
  try {
    writeVersion(root, "1.10.1");
    assert.equal(
      command(root, "git", [
        "add",
        "package.json",
        "package-lock.json",
        "src-tauri/Cargo.toml",
        "src-tauri/tauri.conf.json",
      ]).status,
      0,
    );

    const result = runHook(root);

    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects inconsistent versions even when every version source is staged", () => {
  const root = createRepository();
  try {
    writeVersion(root, "1.10.1");
    writeFileSync(join(root, "package.json"), '{"version":"1.10.2"}\n');
    assert.equal(
      command(root, "git", [
        "add",
        "package.json",
        "package-lock.json",
        "src-tauri/Cargo.toml",
        "src-tauri/tauri.conf.json",
      ]).status,
      0,
    );

    const result = runHook(root);

    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /staged version sources disagree/i);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("validates the staged snapshot rather than unstaged manifest edits", () => {
  const root = createRepository();
  try {
    writeVersion(root, "1.10.1");
    assert.equal(
      command(root, "git", [
        "add",
        "package.json",
        "package-lock.json",
        "src-tauri/Cargo.toml",
        "src-tauri/tauri.conf.json",
      ]).status,
      0,
    );
    writeFileSync(join(root, "package.json"), '{"version":"9.9.9"}\n');

    const result = runHook(root);

    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("allows an AI-only commit without consulting product versions", () => {
  const root = createRepository();
  try {
    writeFileSync(join(root, "AGENTS.md"), "instruction-only change\n");
    assert.equal(command(root, "git", ["add", "AGENTS.md"]).status, 0);
    writeFileSync(join(root, "package.json"), '{"version":"9.9.9"}\n');

    const result = runHook(root);

    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
