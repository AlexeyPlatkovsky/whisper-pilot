import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import {
  access,
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const scriptsDir = path.dirname(fileURLToPath(import.meta.url));
const projectDir = path.dirname(scriptsDir);
const packageJson = JSON.parse(
  await readFile(path.join(projectDir, "package.json"), "utf8"),
);

test("the npm prepare hook is compatible with Ubuntu's POSIX shell", async (t) => {
  const dash = "/bin/dash";
  try {
    await access(dash);
  } catch {
    t.skip("dash is unavailable on this platform");
    return;
  }

  assert.equal(packageJson.scripts.prepare, "sh scripts/install-githooks.sh");
  const fixtureDir = await mkdtemp(path.join(tmpdir(), "whisper-pilot-hooks-"));
  t.after(() => rm(fixtureDir, { force: true, recursive: true }));

  await mkdir(path.join(fixtureDir, "scripts"));
  await copyFile(
    path.join(scriptsDir, "install-githooks.sh"),
    path.join(fixtureDir, "scripts", "install-githooks.sh"),
  );
  execFileSync("git", ["init", "--quiet"], { cwd: fixtureDir });

  const result = spawnSync(dash, ["scripts/install-githooks.sh"], {
    cwd: fixtureDir,
    encoding: "utf8",
  });

  assert.equal(
    result.status,
    0,
    `prepare failed:\n${result.stdout}${result.stderr}`,
  );
  assert.equal(
    execFileSync("git", ["config", "--get", "core.hooksPath"], {
      cwd: fixtureDir,
      encoding: "utf8",
    }).trim(),
    ".githooks",
  );
});

test("npm invokes Bash-only repository scripts with Bash", () => {
  const bashScripts = [
    "version:check",
    "version:sync",
    "version:minor",
    "version:fix",
    "version:feature",
    "version:release",
    "commit:minor",
    "commit:fix",
    "commit:feature",
    "commit:release",
  ];

  for (const name of bashScripts) {
    assert.match(
      packageJson.scripts[name],
      /^bash scripts\//,
      `${name} must honor its target script's Bash shebang`,
    );
  }
});
