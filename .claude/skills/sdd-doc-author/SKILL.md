---
name: sdd-doc-author
description: Authors or updates a single main or extension document in a docs/ SDD tree from the matching template, keeping each document within its ownership boundary. Use when creating or revising idea, architecture, design, testing, roadmap, an ADR, or an extension doc (api, db, security, operations, integrations, glossary).
---

## Scope

- Create or revise exactly one document per run: a main doc (`idea`, `architecture`,
  `design`, `testing`, `roadmap`), one ADR under `decisions/`, or one extension doc.
- Keep the document within the concern it owns per the doc-set convention; link to other
  docs instead of duplicating their content.
- Do not rebuild `INDEX.md` or invent facts — those are separate concerns handled
  outside this skill.

## Required Environment

This skill depends on two files in this repository:
- the `sdd-doc-set` convention at `.claude/conventions/sdd-doc-set.md` (document
  ownership, tiers, extension-doc vocabulary, split rule, ID scheme) — the authority for
  every decision below;
- the matching template under `.claude/sdd/templates/docs/`.

If either is unavailable, report it as a blocker before writing.

Discover target existence at runtime; do not hard-code current document state.

## Inputs

- Target document and whether the run is `new` or `revise`.
- Fixed project tier `Standard`; block any different tier pending a separately
  routed governance change.
- Confirmed source content from the user, repository evidence, or existing docs.
- Manager Route run, tracked-item/lifecycle evidence, and completed Git gate.
- Confirmed plan row ID and plan version for pipeline use, or a stable approved
  source identity for the direct one-document route.
- Author-attempt number, starting at `1` and incremented after invalidating
  rework.

## Procedure

Apply the Stop Conditions throughout; halt and report when any is met.

1. Identify the target document and the concern it owns per the convention.
   Verify that target, mode, confirmed sources, and satisfied dependencies match
   the supplied plan row/version or direct approved-source record; block every
   mismatch.
   Before mutation, verify `new → target absent` and `revise → target present`;
   block a mismatch. If the doc is not part of fixed tier `Standard`, stop and
   report it.
2. Load the matching template and, in `revise` mode, the existing document.
3. Gather confirmed inputs. Do not write inferred project facts; return them as
   pending assumptions for user confirmation.
4. Write or update the document section by section. Keep content inside the doc's ownership
   boundary; when material belongs elsewhere, leave a link or follow-up and use
   a separately routed one-document invocation.
5. For `architecture.md`, apply the convention's split rule without widening
   this run: edit only `architecture.md`. Link to a verified existing extension
   target; when the required extension does not exist, emit a blocked/follow-up
   extension-doc row for a separately routed invocation rather than creating or
   linking to a nonexistent file.
6. Re-read the result and validate its ownership, sources, mode, and template
   completeness. Every required template section must be substantively
   populated or contain an explicitly permitted `N/A`; unresolved placeholders
   block completion. Every completed mutation emits `INDEX sync needed: yes`.

## Stop Conditions

Stop and report a blocker when:
- the document's owning concern is ambiguous or overlaps another doc with no clear owner;
- the requested content conflicts with content already owned by another doc;
- required facts cannot be verified from user input, repository evidence, or existing docs.

## Output Contract

Emit:

`Skill: sdd-doc-author - output below`

Then include:

| Field | Content |
| --- | --- |
| Route / plan / attempt | Manager Route run, plan row and version or approved-source identity, author-attempt number |
| Status | `completed` or `blocked` |
| Document | File written or updated |
| Mode | `new` or `revise` |
| Sections | Sections created or changed |
| Ownership & links | Content moved or linked to other docs, or `none` |
| INDEX sync needed | `yes` for completed; `no — no mutation` for blocked |
| Pending assumptions | `none` for completed; unresolved assumptions for blocked |
| Blockers | Unresolved issues, or `none` |
