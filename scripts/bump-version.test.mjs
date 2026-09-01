import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const repositoryRoot = process.cwd();
const script = join(repositoryRoot, "scripts/bump-version.sh");

function createVersionFixture(version = "1.9.0") {
  const root = mkdtempSync(join(tmpdir(), "whisper-pilot-version-"));
  execFileSync("mkdir", ["-p", join(root, "src-tauri")]);
  writeFileSync(join(root, "src-tauri/Cargo.toml"), `[package]\nversion = "${version}"\n`);
  writeFileSync(
    join(root, "src-tauri/tauri.conf.json"),
    `{\n  "version": "${version}"\n}\n`,
  );
  writeFileSync(join(root, "package.json"), `{\n  "version": "${version}"\n}\n`);
  writeFileSync(
    join(root, "package-lock.json"),
    `{\n  "version": "${version}",\n  "packages": {\n    "": {\n      "version": "${version}"\n    }\n  }\n}\n`,
  );
  return root;
}

function run(root, ...args) {
  return spawnSync(script, args, {
    cwd: root,
    env: { ...process.env, VERSION_ROOT: root },
    encoding: "utf8",
  });
}

function versions(root) {
  return [
    /version = "([^"]+)"/.exec(
      readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8"),
    )[1],
    JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"))
      .version,
    JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version,
    JSON.parse(readFileSync(join(root, "package-lock.json"), "utf8")).version,
  ];
}

test("minor updates every release-version source for a fix", () => {
  const root = createVersionFixture();
  try {
    const result = run(root, "minor");
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(versions(root), ["1.9.1", "1.9.1", "1.9.1", "1.9.1"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("major advances the feature version", () => {
  const root = createVersionFixture();
  try {
    const result = run(root, "major");
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(versions(root), ["1.10.0", "1.10.0", "1.10.0", "1.10.0"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects a bump when release-version sources disagree", () => {
  const root = createVersionFixture();
  try {
    writeFileSync(join(root, "package.json"), '{"version":"0.1.0"}\n');
    const result = run(root, "minor");
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /version sources disagree/i);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("sync makes package metadata match canonical Cargo and Tauri versions", () => {
  const root = createVersionFixture();
  try {
    writeFileSync(join(root, "package.json"), '{"version":"0.1.0"}\n');
    writeFileSync(
      join(root, "package-lock.json"),
      '{"version":"0.1.0","packages":{"":{"version":"0.1.0"}}}\n',
    );
    const result = run(root, "sync");
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(versions(root), ["1.9.0", "1.9.0", "1.9.0", "1.9.0"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
