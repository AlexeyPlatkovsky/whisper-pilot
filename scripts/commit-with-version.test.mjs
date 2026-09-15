import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const repositoryRoot = process.cwd();

function command(root, executable, args, env = {}) {
  return spawnSync(executable, args, {
    cwd: root,
    encoding: "utf8",
    env: { ...process.env, ...env },
  });
}

function createRepository() {
  const root = mkdtempSync(join(tmpdir(), "whisper-pilot-version-commit-"));
  mkdirSync(join(root, ".githooks"), { recursive: true });
  mkdirSync(join(root, "scripts"), { recursive: true });
  mkdirSync(join(root, "src-tauri"), { recursive: true });
  for (const path of [
    ".githooks/pre-commit",
    "scripts/bump-version.sh",
    "scripts/commit-with-version.sh",
    "scripts/check-source-size.mjs",
  ]) {
    copyFileSync(join(repositoryRoot, path), join(root, path));
  }
  writeFileSync(join(root, "scripts/check-ai-instructions.mjs"), "");
  for (const path of [
    ".githooks/pre-commit",
    "scripts/bump-version.sh",
    "scripts/commit-with-version.sh",
  ]) {
    chmodSync(join(root, path), 0o755);
  }
  writeFileSync(join(root, "src-tauri/Cargo.toml"), '[package]\nversion = "1.10.0"\n');
  writeFileSync(
    join(root, "src-tauri/Cargo.lock"),
    '[[package]]\nname = "whisper-pilot"\nversion = "1.10.0"\n',
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
  writeFileSync(
    join(root, "README.md"),
    "![Version](https://img.shields.io/badge/version-1.10.0-purple)\n",
  );
  writeFileSync(join(root, "product.txt"), "baseline\n");
  assert.equal(command(root, "git", ["init", "-q"]).status, 0);
  assert.equal(command(root, "git", ["config", "user.name", "Test"]).status, 0);
  assert.equal(
    command(root, "git", ["config", "user.email", "test@example.com"]).status,
    0,
  );
  assert.equal(command(root, "git", ["config", "core.hooksPath", ".githooks"]).status, 0);
  assert.equal(command(root, "git", ["add", "."]).status, 0);
  assert.equal(
    command(root, "git", ["-c", "core.hooksPath=/dev/null", "commit", "-qm", "baseline"])
      .status,
    0,
  );
  return root;
}

function cargoVersion(root) {
  return /version = "([^"]+)"/.exec(
    readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8"),
  )[1];
}

test("reuses a pre-bumped patch and does not bump an amend", () => {
  const root = createRepository();
  try {
    const bump = command(root, "bash", ["scripts/bump-version.sh", "minor"]);
    assert.equal(bump.status, 0, bump.stderr);
    writeFileSync(join(root, "product.txt"), "small change\n");
    assert.equal(command(root, "git", ["add", "product.txt"]).status, 0);

    const commit = command(root, "bash", [
      "scripts/commit-with-version.sh",
      "minor",
      "-m",
      "small change",
    ]);
    assert.equal(commit.status, 0, commit.stderr);
    assert.equal(cargoVersion(root), "1.10.1");

    writeFileSync(join(root, "product.txt"), "amended change\n");
    assert.equal(command(root, "git", ["add", "product.txt"]).status, 0);
    const amend = command(root, "bash", [
      "scripts/commit-with-version.sh",
      "minor",
      "--amend",
      "--no-edit",
    ]);
    assert.equal(amend.status, 0, amend.stderr);
    assert.equal(cargoVersion(root), "1.10.1");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("release wrapper requires external authorization and safely retries", () => {
  const root = createRepository();
  try {
    writeFileSync(join(root, "product.txt"), "release change\n");
    assert.equal(command(root, "git", ["add", "product.txt"]).status, 0);
    const args = ["scripts/commit-with-version.sh", "release", "-m", "release"];

    const unauthorized = command(root, "bash", args);
    assert.notEqual(unauthorized.status, 0);
    assert.match(unauthorized.stderr, /release requires explicit user authorization/i);
    assert.equal(cargoVersion(root), "1.10.0");

    const authorized = command(root, "bash", args, {
      WHISPERPILOT_RELEASE_AUTHORIZED: "1",
    });
    assert.equal(authorized.status, 0, authorized.stderr);
    assert.equal(cargoVersion(root), "2.0.0");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
