---
name: design-in-pen
description: Create or iterate on a UI design mockup in WhisperPilot's Pencil design file (pencil/*.pen) via the pencil MCP (primary) or the pen CLI (fallback), before or during discussion — never by editing the .pen JSON directly.
---

# Skill: design-in-pen

## When This Skill Applies

Use when a UI design is being explored, sketched, or iterated on in
`pencil/*.pen` — before implementation, as part of a design discussion, or to
produce a mockup for the user to react to. This is the visual-medium
equivalent of `.claude/skills/brainstorm/SKILL.md`: exploratory, no
production code involved, no TaskPilot item required while the touched
content stays unapproved and unconsumed (see §Governance below for the exact
test — unlike `brainstorm`, this skill does mutate a file, so its
trivial-eligibility isn't automatic and depends on that test still holding).

Do not use:
- to implement an already-approved design into real app code — route
  `.claude/skills/sync-pen-code/SKILL.md` (`pen-to-code` direction) instead.
- to update the design file after UI code changed — route
  `.claude/skills/sync-pen-code/SKILL.md` (`code-to-pen` direction) instead.
- for any `.pen` file mutation, however small. There is no trivial-edit
  exception (see §Hard Rule below).

## Hard Rule: Tool-Mediated Mutation Only, No Exceptions

Every mutation to a `pencil/*.pen` file goes through the pencil MCP — or, in
a host where the MCP is not available, the `pen` CLI. Never use `Edit` or
`Write` on the raw JSON, for any reason, including a one-word rename.

This is not a style preference. In this project's own history, a raw
`Edit`-tool JSON change to `pencil/main_view.pen` was silently reverted —
something else (the Pencil app and/or its cloud sync) held the file and
overwrote the direct edit the moment it next saved. Mutating through the MCP
or the CLI is durable because those are paths the owning app/sync expects. A
"trivial" raw edit does not avoid this failure mode; it is exactly what
caused it.

Reading a `.pen` file with `Read` (to understand its current structure before
composing a brief, or to extract facts for `sync-pen-code`) is fine — only
mutation is restricted.

## Required Environment

- **Primary — pencil MCP.** The `mcp__pencil__*` tools are available in the
  host. Before the first mutation, call `get_app_state` with
  `include_canvas_design` and `include_schema` to load the canvas-editor
  instructions and the `.pen` schema, and `get_guidelines` for any
  task-specific guide the mutation needs.
- **Fallback — `pen` CLI.** When the MCP is not available in the host, or its
  environment-level calls fail before any mutation lands (e.g. the Pencil app
  session is unreachable): the `pen` CLI (`pen --help` to confirm it's on
  `PATH`) and an authenticated session (`pen status`). If not authenticated,
  stop and tell the user to run `pen login` themselves (interactive
  email/password or OTP — this cannot be automated), then retry. If neither
  the MCP nor an authenticated `pen` CLI is available, stop with `Status:
  blocked` naming the missing tool path — never fall back to raw file
  mutation.
- Confirm no other process is expected to hold the target file open for
  editing at the same time (ask the user if unsure). The MCP's own Pencil app
  session is the expected holder of the file; the concern is a second
  interactive editing session racing the mutation — a mutation can still lose
  that race the same way a raw edit can.

## Procedure

1. **State the target.** Identify the exact `.pen` file (default
   `pencil/main_view.pen`) and, in `update` mode, read it first with `Read` to
   understand the frames/components already present and their IDs — reuse
   existing reusable components (`"reusable": true` frames) rather than
   duplicating structure, matching this file's existing conventions.
2. **Compose the brief.** Write the design intent as a concrete brief: what
   frame(s)/component(s) to add or change, and the specific visual facts
   (labels, icons, positions, states) — precise enough that a reviewer with no
   other context could check the result against it. Echo the brief back to
   the user before mutating, since it is the instruction the mutation will be
   checked against.
3. **Mutate and render.**
   - **Primary (MCP):** apply the brief with `mcp__pencil__execute` snippets
     against the file (pass its path as `filePath`). Prefer several small,
     verifiable snippets over one large one. When a snippet fails, retry it
     with the `edits`/`editId` patch mechanism instead of resending it
     unchanged. Render the affected frame with `get_screenshot` for your own
     read of the result, and produce the reviewer's PNG with `export_nodes`
     (the affected frame's node ID, PNG format) into a scratch directory
     outside the working tree (e.g. an OS temp directory), so the export
     never dirties the repository.
   - **Fallback (CLI):** mutate and render in one call:
     ```
     pen --in <file> --out <file> --prompt "<brief>" --agent claude \
       --export <preview.png> --export-type png
     ```
     Use the same path for `--in`/`--out` for an in-place edit; use a
     different `--out` only when the user explicitly wants a new file rather
     than an update. Set `--repo` to the project root if the CLI needs it to
     resolve `--in`/`--out`.
4. **Verify before reporting success.**
   Agent: `.claude/agents/pencil-vision-reviewer.md`
   Required input: the produced `export_path` (the `export_nodes` PNG, or the
   CLI `--export` PNG in fallback mode), `comparison_mode: design-intent`,
   `design_intent` set to the exact brief from step 2, and `caller:
   design-in-pen`.
   Required output: `Agent: pencil-vision-reviewer - output below`.
5. **On `Deviations found`**, either refine the brief and repeat from step 3
   (increment an attempt counter, maximum 3), or — if the deviation reveals
   the request itself was ambiguous — stop and ask the user, rather than
   guessing at a fix. After 3 attempts without a `Match`, stop and report to
   the user regardless of ambiguity. Do not report success with unresolved
   deviations.
6. **Sanity-check structure.** After a `Match` verdict, confirm the file is
   still valid JSON with unique element IDs (a quick `python3 -c "import
   json; json.load(open(f))"` and an ID-uniqueness pass is sufficient) — this
   catches a mutation malformation that a visual export might not surface
   (e.g., an off-canvas duplicate).

## Governance

This skill mutates `pencil/*.pen` on every run — a "generated artifact"
effect that `.claude/skills/task-routing/SKILL.md` §Classification excludes
from `trivial` by default. That skill's Classification section also defines
an **exploratory generated-artifact exception**: apply its three criteria to
the specific frame(s)/component(s) this run touches, not to the whole `.pen`
file (a file can hold both settled, already-shipped content and a brand-new
unapproved sketch at once):
1. The touched frame(s) are a design/planning medium, not yet a documented
   authoritative fact — true for a new or still-iterating sketch; false for
   content `.claude/skills/documentation-maintenance/SKILL.md` already treats
   as mirroring shipped UI.
2. The mutation is cheaply reversible via another run of the same tool that
   produced it (an `execute` snippet, or a `pen` CLI call in fallback mode) —
   true for every run of this skill.
3. Nothing yet consumes the touched frame(s)' current state — true until a
   design is approved and handed to `.claude/skills/sync-pen-code/SKILL.md`
   (`pen-to-code`) or `.claude/pipelines/implement-feature.md`.

For a `new`-mode run (a frame that did not exist before this run), criteria 1
and 3 are trivially satisfiable by construction — nothing could have consumed
or documented a frame that didn't exist yet. For an `update`-mode run
touching a **pre-existing** frame, nothing in this file, the `.pen` file's own
fields, or any coupled artifact records per-frame approval/consumption state
— so criteria 1 and 3 are not self-evident and must be actively checked, not
assumed: confirm with the user (or from context already established in the
conversation) whether the touched frame was previously approved and handed to
`sync-pen-code`/`implement-feature.md`. If that cannot be confirmed either
way, do not apply the exception — classify `non-trivial` per
`.claude/skills/task-routing/SKILL.md`'s own default ("when unsure of
complexity, treat as non-trivial").

While all three criteria are confirmed to hold, a `design-in-pen` run is
**trivial**: no TaskPilot item is required, and this skill's own Output
Contract (Brief used, Mutation record, Export, Vision review, Structural
check) is the run's visible record — classification and `task-routing` still
apply as the gate that established this exception applies, but no further
gate ceremony is needed beyond running this skill's own procedure.

The moment criterion 1 or 3 stops holding for a frame — most commonly, once
its design is approved and handed off — classify normally as `non-trivial`
from that point forward for any further work on it. That handoff is exactly
where `.claude/skills/sync-pen-code/SKILL.md`'s `pen-to-code` direction picks
up; its own Governance section states the tracking tier that applies from
there (varies by route — typically the consuming `implement-feature.md`
task's own tier, or Lite when riding on a `documentation-maintenance`
trigger). A prior trivial `design-in-pen` run does not retroactively need its
own TaskPilot item once its output is consumed.

## Output Contract

Begin with:

`Skill: design-in-pen - output below`

| Status | Mode | Target file | Attempt |
|---|---|---|---|

`Status` is `completed` or `blocked`. `Mode` is `new` or `update`. `Attempt`
starts at `1` and increments on a refined re-run.

Then emit:
- **Brief used** — the exact design brief applied in step 3 (in CLI fallback
  mode, the exact text sent to `pen --prompt`).
- **Mutation record** — the `execute` snippets applied (or a faithful summary
  of them), or the exact CLI command run in fallback mode.
- **Export** — the produced PNG path.
- **Vision review** — the `pencil-vision-reviewer` verdict and a link/summary
  of its output.
- **Structural check** — valid JSON / invalid JSON (with error) and
  ID-uniqueness pass/fail.
- **Next step** — `ready for discussion`, `ready to hand to sync-pen-code
  (pen-to-code) once approved`, or the specific blocker.

`blocked` requires the exact missing input, failed auth, an unavailable
mutation tool (neither MCP nor authenticated `pen` CLI), or unresolved
deviation, and does not claim a mutation succeeded.
