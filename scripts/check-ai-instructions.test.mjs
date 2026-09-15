import assert from "node:assert/strict";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const repositoryRoot = process.cwd();
const sandboxes = new Map([
  ["code-reviewer", "read-only"],
  ["explorer", "read-only"],
  ["instruction-evaluator", "read-only"],
  ["requirements-reviewer", "read-only"],
  ["sdd-auditor", "read-only"],
  ["visual-reviewer", "read-only"],
  ["test-author", "workspace-write"],
  ["test-runner", "workspace-write"],
  ["unit-test-author", "workspace-write"],
]);
const skills = [
  "brainstorm",
  "bug-triage",
  "documentation-maintenance",
  "pencil-design",
  "requirements-discovery",
  "sdd-docs",
  "taskpilot-work",
  "testing",
];

function agent(role = "unit-test-author", effort = "medium") {
  return `name = "${role}"
description = "Write focused tests."
sandbox_mode = "${sandboxes.get(role)}"
model = "gpt-5.6-luna"
model_reasoning_effort = "${effort}"
developer_instructions = """
Responsibility: tests.
Isolation reason: bounded test work.
"""
`;
}

function fixture(content) {
  const root = mkdtempSync(join(tmpdir(), "whisper-pilot-ai-lint-"));
  mkdirSync(join(root, ".codex/agents"), { recursive: true });
  mkdirSync(join(root, "scripts"), { recursive: true });
  copyFileSync(
    join(repositoryRoot, "scripts/check-ai-instructions.mjs"),
    join(root, "scripts/check-ai-instructions.mjs"),
  );
  writeFileSync(join(root, "AGENTS.md"), "# Test contract\n");
  writeFileSync(
    join(root, ".codex/config.toml"),
    '[agents]\nmax_concurrent_threads_per_session = 8\ndefault_subagent_model = "gpt-5.6-luna"\ndefault_subagent_reasoning_effort = "medium"\n',
  );
  for (const role of sandboxes.keys()) {
    writeFileSync(
      join(root, `.codex/agents/${role}.toml`),
      role === "unit-test-author" ? content : agent(role),
    );
  }
  for (const skill of skills) {
    const directory = join(root, ".agents/skills", skill);
    mkdirSync(directory, { recursive: true });
    writeFileSync(
      join(directory, "SKILL.md"),
      `---\nname: ${skill}\ndescription: Test ${skill} capability.\n---\n`,
    );
  }
  assert.equal(spawnSync("git", ["init", "-q"], { cwd: root }).status, 0);
  return root;
}

function run(root, staged = false) {
  const args = ["scripts/check-ai-instructions.mjs"];
  if (staged) args.push("--staged");
  return spawnSync("node", args, {
    cwd: root,
    encoding: "utf8",
  });
}

test("accepts an available Luna effort and syntactically valid agent TOML", () => {
  const root = fixture(agent());
  try {
    assert.equal(run(root).status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects an effort unavailable for Luna", () => {
  const root = fixture(agent("unit-test-author", "ultra"));
  try {
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /reasoning effort is incompatible/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects malformed TOML even when required field text is present", () => {
  const malformed = agent().replace(
    'description = "Write focused tests."',
    'description = "unterminated',
  );
  const root = fixture(malformed);
  try {
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /TOML parse failed/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("does not accept required agent fields hidden inside developer instructions", () => {
  const hiddenModel = agent()
    .replace('model = "gpt-5.6-luna"\n', "")
    .replace("Responsibility: tests.", 'model = "gpt-5.6-luna"\nResponsibility: tests.');
  const root = fixture(hiddenModel);
  try {
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /missing model/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("parses config TOML instead of trusting matching field text", () => {
  const root = fixture(agent());
  try {
    writeFileSync(
      join(root, ".codex/config.toml"),
      '[agents]\nmax_concurrent_threads_per_session = 8\ndefault_subagent_model = "gpt-5.6-luna"\ndefault_subagent_reasoning_effort = "medium"\nbroken = "unterminated\n',
    );
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /TOML parse failed/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects removal of a required reviewer agent", () => {
  const root = fixture(agent());
  try {
    rmSync(join(root, ".codex/agents/code-reviewer.toml"));
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /code-reviewer\.toml: required agent is missing/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects removal of a required project skill", () => {
  const root = fixture(agent());
  try {
    rmSync(join(root, ".agents/skills/testing/SKILL.md"));
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /testing\/SKILL\.md: required project skill is missing/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects a staged deletion from the required agent inventory", () => {
  const root = fixture(agent());
  try {
    assert.equal(spawnSync("git", ["add", "."], { cwd: root }).status, 0);
    assert.equal(
      spawnSync(
        "git",
        [
          "-c",
          "user.name=Test",
          "-c",
          "user.email=test@example.com",
          "commit",
          "-qm",
          "baseline",
        ],
        { cwd: root },
      ).status,
      0,
    );
    rmSync(join(root, ".codex/agents/code-reviewer.toml"));
    assert.equal(
      spawnSync("git", ["add", "-u", ".codex/agents"], { cwd: root }).status,
      0,
    );
    const result = run(root, true);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /code-reviewer\.toml: required agent is missing/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
