---
name: work-with-git
description: Manage git branch selection for WhisperPilot tasks and report the commit/push boundary.
---

# Skill: work-with-git

## Purpose

Keep WhisperPilot work on a task-specific git branch before edits begin.

## When This Skill Applies

Use this skill when the manager routes non-trivial work before edits begin, immediately before any merge into canonical branch `main`, or when the user asks about task branch strategy.

For trivial work, skip this skill; branch behavior follows `AGENTS.md`
§Git Operation Authority.

## Branch Rules

- Use the task identity and Git-operation decisions authorized by `AGENTS.md`; this skill only applies them to branch selection and reporting.
- For non-trivial tasks, proceed only with an authorized branch decision; if none exists, stop and ask.
- Create new task branches only from `origin/main` unless the user explicitly names another base.
- If the task continues related work on a branch created before the task-ID rule and its uncommitted changes match the task, keep working on it and report a legacy branch exception.
- For ID-bearing work, keep working on an existing task branch only when its name contains the task ID and its uncommitted changes match the task.
- For instruction-system changes, apply the same existing-task-branch rule as other ID-bearing work.
- It is acceptable to perform a child task on a branch named for its parent feature or epic when the branch clearly covers the requested work.
- If the current branch or uncommitted changes appear unrelated to the requested task, stop and ask the user whether to create a new branch or stay on the current branch.
- If an ID-bearing task needs a new branch, name it `<kind>/vp-<number>-short-task-slug`, with no tool prefix. Use a lowercase TaskPilot ID and a kebab-case slug of 3-6 meaningful words. Map TaskPilot types to `<kind>` as follows: `bug` → `bug`, `feature` → `feat`, and `task` or `epic` → `task`. For example: `feat/vp-01-introduce-local-ai`.
- Apply `AGENTS.md` §Git Operation Authority for branch publication and every Git mutation.
- Preserve user changes and follow the destructive-action rules in `AGENTS.md`.

## Commit And Push Rules

The commit/push boundary is owned by `AGENTS.md`; this skill only formats task-scoped commit messages and reports the boundary.

- When a task has an ID, every commit message must begin `<TASKPILOT-ID>: `, for example `VP-17: `.
- Every non-trivial change uses its existing TaskPilot ID in the commit-message prefix.
- After completing work that changed files, suggest one short imperative commit message, ID-prefixed only when an ID exists.

## Recommended Checks

Before edits:

1. Confirm the identity decision required by `AGENTS.md` §Task Identity And Tracking.
2. Inspect `git status --short --branch`.
3. If a new branch is needed, run `git fetch origin main` before branch creation when network access is available.
4. If fetching fails but local `origin/main` exists, create the approved branch from local `origin/main` and report the fetch failure in this skill's output.
5. If `origin/main` is missing, stop and ask the user for the branch base.
6. Check whether uncommitted changes are present and whether they appear related to the task.
7. If publication is authorized under `AGENTS.md` and a new branch was created, verify remote tracking. Report any failure in the Remote Published column; otherwise report publication as skipped.

After edits:

1. Inspect `git status --short`.
2. For ID-bearing work, before each commit and immediately before merge into `main`, compare the provisional ID with the registry and current local/fetched-remote task branches. The first task merged into `main` keeps a colliding ID; block each later merge into `main` until that task takes the next unused ID and updates its branch and artifacts. Intermediate branch merges do not finalize IDs. Prior provisional commit messages remain a documented collision exception.
3. Report changed files and whether anything remains unstaged or uncommitted.
4. Suggest a commit message when files changed.

## Output Contract

When this skill gates non-trivial routed work, begin with:

`Skill: work-with-git - output below`

Then report:

| Status | Branch Decision | Base | Remote Published | Commit / Push Boundary |
|--------|-----------------|------|-----------------|------------------------|

`Status` must be one of: `completed`, `skipped`, or `blocked`.
`Remote Published` must be one of: `Yes (push succeeded)`, `No (push failed — see reason)`, `No (skipped — task continues on existing branch)`, or `No (skipped — publication not approved)`.
