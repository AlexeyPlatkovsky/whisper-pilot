---
name: sdd-index-sync
description: Rebuilds the generated row sets in docs/INDEX.md (document map, feature registry, decision log) from the current docs tree so the index reflects the files that actually exist. Use after any doc, feature, or ADR change.
---

## Scope

- Regenerate the row set of the three marked regions in `docs/INDEX.md` only, from
  the present state of the docs tree. This is not a full-file rewrite.
- Register the docs that exist, the feature folders with their counts, and the ADRs.
- **Never** rewrite a curated cell of a retained row, and never touch any text
  outside the markers.
- Do not edit any document other than `INDEX.md`, invent statuses, or add routing, gates,
  or behavioral rules to the index.

## Required Environment

This skill depends on files in this repository:
- the `sdd-doc-set` convention at `.claude/conventions/sdd-doc-set.md` (what belongs in
  the index and the ID scheme);
- the `INDEX.md` template at `.claude/sdd/templates/docs/INDEX.md`.

If the docs root, convention, or template is missing, unreadable, or malformed,
report it as a blocker.

## Inputs

- The docs root: `docs/` at the WhisperPilot repository root.
- Mode `create` when `docs/INDEX.md` is absent or `update` when it exists.
- The exact manager Route run and sync-attempt number. Start at `1`; increment
  after a retry or documentation rework invalidates a prior sync; every attempt
  after `1` also requires the previous labeled sync artifact.

## Procedure

Apply the Stop Conditions throughout; halt and report when any is met.

1. Scan the docs root and enumerate the registrable key set defined in
   §Registrable keys. Register only files that exist; do not treat build output,
   test results, or TaskPilot records as docs.
2. Scan `features/` for `F<NNN>_*` folders; for each, derive the active
   requirement, task, and scenario ID ranges. Exclude rows carrying the exact
   `Superseded: yes — <replacement ID or reason>` marker in the
   Requirement/Task cell, or immediately below a scenario heading, from active
   counts; validate that their IDs remain present, unique, and unreused. Do not
   infer or copy TaskPilot status into the index.
3. Scan `decisions/` for `ADR-*` files and read each status.
4. Validate the complete scanned source, ID/supersession rules, required feature
   files, folder names, and the prepared source-to-index render before mutation.
   There is no permitted-status enum: validate only that each ADR yields a
   non-empty status token per §Derived-cell rendering, and never reject a status
   because it is unfamiliar (`partially superseded` is a real one in this repo).
5. In `create` mode, require the index to be absent and render it from the
   template with its generated markers, dropping every comment block that opens
   with `TEMPLATE GUIDANCE` — those are instructions to the creator, not content
   of a product doc. Keep the marker comments and each table's header row. In `update` mode, require the index to
   exist and replace only sections delimited by unique generated markers.
   A mode/existence mismatch or missing/duplicated update marker blocks.
   Replace a marked region by the **key-preserving row merge** below, never by
   discarding it and re-rendering from the template.
6. Capture each region's exact pre-write text before mutating. Then re-read the
   result and prove one-to-one correspondence with the scanned tree while
   preserving curated sections: every preserved cell of every retained row must
   be byte-identical to its captured value, and no text outside the markers may
   have changed. The captured pre-write text is the baseline, not HEAD: a
   `git diff` on the index is supporting evidence only, and any change already
   uncommitted there before this run is excluded from the comparison. A
   post-write failure is `recovery required`; report the actual index state and
   exact recovery action.

## Generated Markers And The Key-Preserving Row Merge

This section is normative. Each regenerated table sits between a matched pair of
unique markers, each marker on its own line, with the region content between them:

```
<!-- sdd-index-sync:begin documents -->
<!-- sdd-index-sync:end documents -->
<!-- sdd-index-sync:begin features -->
<!-- sdd-index-sync:end features -->
<!-- sdd-index-sync:begin decisions -->
<!-- sdd-index-sync:end decisions -->
```

A marker is recognized only as an **exact full-line comment literal**; a mention
of the marker name inside prose is not a marker. Each region must contain
exactly one GFM table with a header row and nothing else; any other content
blocks.

The sync owns **which rows exist**. It does not own curated cells:

| Region | Row key | Cells the sync derives | Cells it preserves |
| --- | --- | --- | --- |
| `documents` | document filename or folder name | the key cell only | every other cell |
| `features` | feature ID (`F<NNN>`) | the key cell plus the requirement, task, and scenario ID-range cells | every other cell |
| `decisions` | ADR ID | the key cell only | every other cell |

### Registrable keys

The key set is closed. Do not infer registrability from naming vocabulary:

- `documents` — every `*.md` at the docs root except `INDEX.md`, plus every
  docs-root subfolder that contains at least one `.md` and is not `features/`
  (registered with a trailing `/`). A document outside
  `.claude/conventions/sdd-doc-set.md`'s recognized extension vocabulary is
  **still registered**; that vocabulary is naming guidance and never suppresses
  registration of a file that exists.
- `features` — every `F<NNN>_*` folder under `features/`.
- `decisions` — every `ADR-*` file under `decisions/`.

### Derived-cell rendering

- Locate a derived cell by **exact column header text** (`Requirements`, `Tasks`,
  `Scenarios`), never by column position — a region may carry extra curated
  columns in any order. A missing expected header blocks.
- Render a key cell as a backtick code span in `documents` (a folder keeps its
  trailing `/`), and bare in `features` and `decisions`. Strip surrounding
  backticks and whitespace before comparing a key against an existing row.
- Render an ID range as `<prefix><lowest>–<highest>` (en dash) when the active
  IDs are contiguous, and as a comma-separated ascending list otherwise.
  `<prefix>` is the bare ID-type letter — `R`, `T`, or `S` — without the
  `F<NNN>-` qualifier: `R1–R6`, `T1, T2, T4`. Apply the same rule to every
  ID-range cell.
- For a new `decisions` row only, seed `Title` from the ADR's H1 with any
  `ADR-NNN: ` prefix stripped, and `Status` from its **status token**: the text
  after `**Status:**` up to the first `(`, `—`, or end of line, trimmed. On a
  retained row both stay preserved; report under `Structural/index gaps` only
  when that token is not a case-insensitive prefix of the preserved cell, so a
  curated elaboration of a matching status is not flagged run after run.

### Merge rules

- A key present in both the tree and the existing region keeps that row's
  preserved cells **byte-identical**. Never reword, re-derive, or normalize them.
- A key present in the tree but absent from the region is appended with its
  derived cells filled and each preserved cell set to the literal `TODO`, so the
  gap is visible rather than silently plausible.
- A key present in the region but absent from the tree is dropped; name each
  dropped key in the output artifact.
- When one run both drops and adds a key in the same region, report
  `possible rename: <dropped> → <added>` so a human can restore the curated
  cells. Never auto-carry them.
- Row order within a region is preserved for existing keys; new rows append in
  ascending lexicographic order by key. Do not re-sort a region that a human has
  ordered deliberately.
- A region whose header row differs from the template's keeps its own header;
  extra curated columns are preserved content, not drift to be corrected.
- In `create` mode, render the template and then apply these same rules against
  its seeded rows: a seeded key with no matching file is dropped, and a seeded
  key that does match keeps the template's text as its initial curated cells.

Anything outside the markers — prose, headings, notes sections, the tier line —
is never read for merge purposes and never rewritten.

## Stop Conditions

Stop and report a blocker when the docs root cannot be located or is not a recognizable
SDD doc tree.

## Output Contract

Emit:

`Skill: sdd-index-sync - output below`

Then include:

| Field | Content |
| --- | --- |
| Route run / attempt | Exact manager Route run and sync-attempt number |
| Mode | Exact `create` or `update` mode used |
| Status | `completed` after verified write, `blocked` before mutation, or `recovery required` after a failed post-write check |
| Docs registered | Count and names |
| Features registered | Count and IDs |
| ADRs registered | Count and IDs |
| Rows added / dropped | Keys appended (with their `TODO` cells), keys dropped, and any `possible rename` pairs, or `none` |
| Curated content preserved | `pass`, or `fail — <region>:<key>:<column>`; the byte-identical check on retained rows' preserved cells and on all text outside the markers, with its observable `git diff` evidence |
| Structural/index gaps | ID, required-file, marker, or source-to-index gaps; every region/key/column still holding a literal `TODO`; every preserved ADR `Status` disagreeing with its file; or `none`. Full link traceability is not assessed |
| Blockers | Unresolved issues, or `none` |
| Validation | Source-to-index comparison and curated-content preservation result |

For `recovery required`, include the actual `docs/INDEX.md` state and exact
recovery action. Callers stop downstream work, preserve the tree, increment the
sync attempt with the prior artifact, and rerun after recovery.
