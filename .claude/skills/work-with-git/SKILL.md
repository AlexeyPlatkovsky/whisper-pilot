---
name: work-with-git
description: Manage git branch selection for WhisperPilot tasks and report the commit/push boundary.
---

# Skill: work-with-git

## Purpose

Enforce the user-authorized branch decision and task-scoped commit boundary.

## When This Skill Applies

Use this skill when the manager routes non-trivial work before edits begin,
immediately before an authorized merge into canonical branch `main`, or when the
user asks about task branch strategy. A branch-strategy question uses
`advisory-only` mode and performs no Git mutation.

For trivial work, skip this skill; branch behavior follows `AGENTS.md`
§Git Operation Authority.

## Branch Rules

- Use the task identity and Git-operation decisions authorized by `AGENTS.md`; this skill only applies them to branch selection and reporting.
- For non-trivial tasks, proceed only with an authorized branch decision; if none exists, stop and ask.
- Create new task branches only from `origin/main` unless the user explicitly names another base.
- If the task continues related work on a branch created before the task-ID rule and its uncommitted changes match the task, keep working on it and report a legacy branch exception.
- For ID-bearing work, keep working on an existing task branch only when its name contains the task ID and its uncommitted changes match the task.
- For TaskPilot-exempt instruction-system work, use the explicitly approved
  current branch or an explicitly approved descriptive branch; do not require
  or invent a TaskPilot ID.
- It is acceptable to perform a child task on a branch named for its parent feature or epic when the branch clearly covers the requested work.
- On a feature/epic batch branch, the branch name does not make the parent the
  active work item. The routed batch manifest must identify the one active child
  task (or the explicitly declared two-task delivery cohort) before edits.
- If the current branch or uncommitted changes appear unrelated to the requested task, stop and ask the user whether to create a new branch or stay on the current branch.
- If an ID-bearing task needs a new branch, name it
  `<kind>/<lowercase-taskpilot-id>-<3-to-6-word-kebab-slug>`, with no tool
  prefix. Map types as follows: `bug` → `bug`, `feature` → `feat`, and `task`
  or `epic` → `task`. Example: `feat/wp-17-introduce-local-ai`.
- Never publish a branch unless the user explicitly requests that push in the current instruction. Branch-creation approval does not authorize an initial `git push -u`, and no pipeline or task lifecycle step may infer push authority.
- Apply `AGENTS.md` §Git Operation Authority for branch publication and every Git mutation.
- Preserve user changes and follow the destructive-action rules in `AGENTS.md`.
- When a branch has a configured upstream, verify the upstream remote branch name
  matches the local branch name (`origin/<current-branch>`). If the upstream
  points to a different branch (e.g., `origin/main`), stop and block — the
  tracking must be corrected before work proceeds. Report the mismatch in the
  output table as `blocked` with the reason.

## Commit And Push Rules

The commit/push boundary is owned by `AGENTS.md`; this skill formats and verifies
the required task-scoped local commits.

- A tracked implementation task's local commit must include both its code and
  every related TaskPilot item/comment/lifecycle file. Prepare and verify the
  `done` lifecycle record first, stage it with the code, then make the single
  task-scoped local commit. Report that hash in closure evidence without a
  post-commit TaskPilot write.
- In a normal delivery, one task has one local commit and every commit message
  begins `<TASKPILOT-ID>: `, for example `WP-17: `.
- A two-task delivery cohort is the only multi-task commit exception. Its commit
  message begins `<TASKPILOT-ID>, <TASKPILOT-ID>: `, for example
  `WP-17, WP-18: complete transcription persistence flow`; both IDs must also
  appear in both items' completion evidence.
- Do not start a next sibling task until the prior task has its required local
  commit and verified `done` lifecycle artifact. A declared delivery cohort may
  start its two members together, but no third task may start.
- AI-governance maintenance has no TaskPilot completion-commit requirement and
  follows the explicit commit authority in `AGENTS.md`.

## Recommended Checks

Before edits:

1. Confirm the identity decision required by `AGENTS.md` §Task Identity And Tracking.
2. Inspect `git status --short --branch`.
3. Verify upstream tracking: if the branch has a configured upstream, confirm it
   points to `origin/<current-branch>` (not `origin/main` or any other branch).
   If the upstream is missing and a push has not yet been authorized, note it;
   if the upstream is wrong, block with the mismatch reason per §Branch Rules.
4. If a new branch is needed, run `git fetch origin main` before branch creation when network access is available.
5. If fetching fails but local `origin/main` exists, create the approved branch from local `origin/main` and report the fetch failure in this skill's output.
6. If `origin/main` is missing, stop and ask the user for the branch base.
7. Check whether uncommitted changes are present and whether they appear related to the task.
8. If publication is authorized under `AGENTS.md` and a new branch was created, verify remote tracking: confirm the upstream is set to `origin/<branch-name>` and the remote branch exists. Report any failure in the Remote Published column; otherwise report publication as skipped.

After edits:

1. Inspect `git status --short`.
2. For ID-bearing work, before each commit and authorized merge, verify the
   canonical TaskPilot ID against the registry. Any duplicate or inconsistent
   identity is a blocker returned to `taskpilot-work`; this skill never assigns
   or renumbers TaskPilot IDs.
3. Report changed files and whether anything remains unstaged or uncommitted.
   Before committing, enumerate task-scoped paths, stage only those paths, and
   inspect `git diff --cached --name-status`. Block if any staged path is
   unrelated to the manager-declared scope.
4. For tracked implementation work, verify the `in_progress → done` TaskPilot
   transition first, then stage its finalized TaskPilot records with the code
   and create the required local commit. Report the commit hash in closure
   evidence without a post-commit TaskPilot write. For AI-governance work,
   report whether a commit was requested or remains uncommitted.

### Commit-failure recovery

If a local commit fails after a tracked item's verified `done` transition, do
not run task-complete or make another TaskPilot write. Emit `Local commit
evidence — failed` with the Git error and `git status --short` output. Resolve
only the staging, Git, hook, or environment cause without changing task code or
TaskPilot records, then re-stage the unchanged task scope and retry the same
local commit. If recovery requires a task-code, test, or TaskPilot-record
change, stop and report an atomicity blocker; a coordinator must resolve the
completed lifecycle state before implementation can resume.

## Output Contract

When this skill gates non-trivial routed work, begin with:

`Skill: work-with-git - output below`

Then report:

| Status | Branch Decision | Base | Remote Published | Commit / Push Boundary |
|--------|-----------------|------|-----------------|------------------------|

`Status` must be one of: `completed`, `skipped`, or `blocked`.
`Remote Published` must be one of: `Yes (push succeeded)`, `No (push failed — see reason)`, `No (skipped — task continues on existing branch)`, or `No (skipped — publication not approved)`.

For an authorized local commit, emit:

`Local commit evidence - output below`

| Status | Task identity | Staged paths verified | Commit hash | Uncommitted remainder | Push |
|---|---|---|---|---|---|
| completed / blocked / failed | `<ID(s)>` / exempt | yes / no — reason | `<hash>` / none | `<paths>` / none | skipped / completed / failed |
