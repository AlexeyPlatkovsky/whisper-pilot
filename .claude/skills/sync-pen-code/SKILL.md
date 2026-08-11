---
name: sync-pen-code
description: Bidirectional sync between WhisperPilot's Pencil design source (pencil/*.pen) and the real React/Rust UI code. Direction pen-to-code translates an approved design into implementer-ready facts; direction code-to-pen updates the design file after UI code changed, via the pencil MCP (primary) or the pen CLI (fallback).
---

# Skill: sync-pen-code

## When This Skill Applies

Use in one of two directions, never both in the same invocation:

- **`pen-to-code`** — routed as a required early step inside
  `.claude/pipelines/implement-feature.md` (its Step 1a) whenever the task's
  target surface is UI-visual and an approved design exists in
  `pencil/*.pen`. This direction never writes app code by itself — it only
  reads the `.pen` file and produces a translation. A request to *implement*
  an approved design into working code always routes to
  `.claude/pipelines/implement-feature.md` (`task-routing`'s precedence rule
  3), never to a standalone run of this direction; standalone `pen-to-code`
  is for producing the translation alone (e.g. answering "what does the
  approved design say" without building it).
- **`code-to-pen`** — routed as an extension of
  `.claude/skills/documentation-maintenance/SKILL.md`'s staleness trigger,
  after UI-visual code changed, so `pencil/*.pen` doesn't silently drift from
  what shipped. Also route directly when the user explicitly asks to sync the
  design file with current code (a drift correction, not an implementation
  request).

Do not use:
- for pure design exploration with no code counterpart yet — route
  `.claude/skills/design-in-pen/SKILL.md` instead.
- for a `.pen` structural fix unrelated to a code change (e.g. renumbering
  component IDs, fixing a broken `ref`) — route `design-in-pen`. There is no
  third "trivial fix" path that bypasses tool-mediated mutation; see that
  skill's Hard Rule.

## Hard Rule: Tool-Mediated Mutation Only, No Exceptions

Every mutation to a `pencil/*.pen` file (the `code-to-pen` direction) goes
through the pencil MCP — or, in a host where the MCP is not available, the
`pen` CLI. Never use `Edit` or `Write` on the raw JSON — see
`.claude/skills/design-in-pen/SKILL.md` §Hard Rule for the incident that
established this rule; it applies identically here. Reading a `.pen` file
with `Read` is fine in either direction — only mutation is restricted, and
`pen-to-code` never mutates at all (see its Procedure step 2 below for the
one render it may optionally produce, which is structurally read-only).

## Required Environment

- **Primary — pencil MCP.** The `mcp__pencil__*` tools are available in the
  host. Before the first mutation, call `get_app_state` with
  `include_canvas_design` and `include_schema` to load the canvas-editor
  instructions and the `.pen` schema, and `get_guidelines` for any
  task-specific guide the mutation needs.
- **Fallback — `pen` CLI.** When the MCP is not available in the host, or its
  environment-level calls fail before any mutation lands (e.g. the Pencil app
  session is unreachable): the `pen` CLI on `PATH` and an authenticated
  session (`pen status`). If not authenticated, stop and tell the user to run
  `pen login` themselves, then retry. If neither the MCP nor an authenticated
  `pen` CLI is available, stop with `Status: blocked` naming the missing tool
  path — never fall back to raw file mutation.
- `code-to-pen` only: confirm no other process is expected to hold the target
  file open for editing at the same time (ask the user if unsure). The MCP's
  own Pencil app session is the expected holder of the file; the concern is a
  second interactive editing session racing the mutation.

## Procedure — `pen-to-code`

This direction never mutates the `.pen` file; it only reads.

1. **Read the approved frame(s).** Use `Read` on the target `.pen` file to
   locate the frame(s)/component(s) that define the approved design for this
   task. Do not resolve an open design question here — if the design is not
   yet approved (no confirmed `design-in-pen` output or user confirmation),
   stop and route `design-in-pen` or `.claude/skills/brainstorm/SKILL.md`
   first.
2. **Optionally render for a visual read.**
   - **Primary (MCP):** export the approved frame(s) with
     `mcp__pencil__export_nodes` (the frame node IDs, PNG format) into a
     scratch directory outside the working tree (e.g. an OS temp directory),
     so the export never dirties the repository. This is a pure export — no
     mutation of the source file is involved.
   - **Fallback (CLI):** `pen` requires `--prompt` on every invocation (there
     is no prompt-free pure-export mode), so a read-only render uses a
     neutral no-op prompt against a **scratch output path distinct from the
     source file** — never the same path for `--in` and `--out` here, since
     compliance with a "do not change anything" prompt is the mutating
     agent's best effort, not a structural guarantee, and this direction must
     never risk the source file:
     ```
     pen --in <file> --out <scratch-copy.pen> --prompt "Do not change anything; render only." --export <preview.png>
     ```
   Skip this entirely if the JSON facts alone are sufficient — most
   `pen-to-code` runs should skip it.
3. **Produce a translation.** Extract concrete, implementer-ready facts from
   the frame(s): layout structure, exact copy/labels, icon names, states
   (active/inactive, hover, disabled), and any reusable-component references
   the frame uses. This translation becomes input to
   `.claude/skills/testing-pro/SKILL.md` and
   `.claude/skills/implement-tauri-feature/SKILL.md` in the calling pipeline
   — it does not itself write app code.
4. **Do not verify against a build here.** The implementation doesn't exist
   yet at this point in the pipeline; there is nothing to compare a render
   against. Post-implementation visual verification is
   `implement-feature.md` Step 4 (manual UI verification), which may invoke
   `.claude/agents/pencil-vision-reviewer.md` in `counterpart-image` mode
   (comparing a build screenshot against this step's export) if — and only
   if — step 2 above actually produced one.

## Procedure — `code-to-pen`

1. **Gather the change.** When triggered by `documentation-maintenance`, take
   the exhaustive changed-file list and a concrete description of what
   changed, visually, from the implementation artifact that triggered this
   (not a guess from file names alone). When invoked directly with no
   triggering implementation artifact (the standalone `task-routing` entry
   point), instead inspect the current `git diff`/working tree for the
   affected UI files and confirm the resulting visual-delta description with
   the user before composing the brief — do not guess from file names alone
   here either.
2. **Compose the brief.** State the exact visual delta as a brief a reviewer
   with no other context could check the result against (e.g. "the header
   action group lost its Re-run icon and gained a Copy icon between Craft and
   Export"), naming the affected frame(s).
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
4. **Verify before reporting success.**
   Agent: `.claude/agents/pencil-vision-reviewer.md`
   Required input: the produced `export_path` (the `export_nodes` PNG, or the
   CLI `--export` PNG in fallback mode), `comparison_mode:
   counterpart-image`, `counterpart_path` set to a current screenshot of the
   running app for the affected view, and `caller: sync-pen-code /
   code-to-pen`. If no current screenshot is available, obtain one (e.g. via
   the manual UI verification already run for the triggering change) rather
   than skipping the check — do not report success with an unverified
   mutation.
5. **On `Deviations found`**, refine the brief and repeat from step 3
   (increment an attempt counter, maximum 3). After 3 attempts without a
   `Match`, stop and report the gap to the user.
6. **Sanity-check structure** — valid JSON, unique IDs — same as
   `design-in-pen` step 6.

## Governance

`pen-to-code` produces no independent artifact commitment of its own — the
task it feeds is tracked (TaskPilot, tiers, gates) exactly as
`implement-feature.md` already requires. `code-to-pen`, when triggered by
`documentation-maintenance`, is tracked at **Lite tier** per `AGENTS.md`
§Quality Tiers (visual-only, no runtime behavior change) rather than
requiring its own separate TaskPilot item — it rides on the TaskPilot item of
the implementation change that triggered it. When either direction is routed
directly (the standalone `task-routing` entry point, no triggering task to
ride on), it is classified and tracked normally as its own **Lite tier**
non-trivial task per `AGENTS.md` §Quality Tiers.

## Output Contract

Begin with:

`Skill: sync-pen-code - output below`

| Status | Direction | Target file | Attempt |
|---|---|---|---|

`Status` is `completed` or `blocked`. `Direction` is `pen-to-code` or
`code-to-pen`. `Attempt` starts at `1`, increments on a refined re-run.

Then, for `pen-to-code`, emit:
- **Source frame(s)** — the `.pen` frame/component IDs read.
- **Translation** — the extracted implementer-ready facts (layout, copy,
  icons, states, reusable-component references).
- **Export** — the PNG path from procedure step 2, or `not rendered` when
  that optional step was skipped. `implement-feature.md` Step 4 only attempts
  its optional `pencil-vision-reviewer` check when this is a path.
- **Not yet verified** — explicit note that post-implementation visual
  verification is deferred to the calling pipeline's later steps.

For `code-to-pen`, emit:
- **Brief used** — the exact visual-delta brief applied in step 3 (in CLI
  fallback mode, the exact text sent to `pen --prompt`).
- **Mutation record** — the `execute` snippets applied (or a faithful summary
  of them), or the exact CLI command run in fallback mode.
- **Export** — the produced PNG path.
- **Vision review** — the `pencil-vision-reviewer` verdict and a link/summary
  of its output.
- **Structural check** — valid JSON / invalid JSON (with error) and
  ID-uniqueness pass/fail.

`blocked` requires the exact missing input, unapproved design, failed auth,
an unavailable mutation tool (neither MCP nor authenticated `pen` CLI), or
unresolved deviation, and does not claim a mutation succeeded.
