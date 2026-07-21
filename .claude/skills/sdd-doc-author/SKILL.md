---
name: sdd-doc-author
description: Authors or updates a single main or extension document in a docs/ SDD tree from the matching template, keeping each document within its ownership boundary. Use when creating or revising idea, architecture, design, testing, roadmap, an ADR, or an extension doc (api, db, security, operations, integrations, glossary).
---

## Scope

- Create or revise exactly one document per run: a main doc (`idea`, `architecture`,
  `design`, `testing`, `roadmap`), one ADR under `decisions/`, or one extension doc.
- Keep the document within the concern it owns per the doc-set convention; link to other
  docs instead of duplicating their content.
- Do not create or edit feature folders, rebuild `INDEX.md`, or invent facts — those are
  separate concerns handled outside this skill.

## Required Environment

This skill depends on two files in this repository:
- the `sdd-doc-set` convention at `.claude/conventions/sdd-doc-set.md` (document
  ownership, tiers, extension-doc vocabulary, split rule, ID scheme) — the authority for
  every decision below;
- the matching template under `.claude/sdd/templates/docs/`.

If either is unavailable, report it as a blocker before writing.

WhisperPilot's docs root is `docs/`; `idea.md` and `architecture.md` already exist there,
so runs against them are `revise`, not `new`. There is no `design-book.md` yet — UI design
detail belongs in `design.md` until a design system large enough to warrant its own
extension doc emerges.

## Inputs

- Target document and whether the run is `new` or `revise`.
- The project tier (`Lean`, `Standard`, `Full`) when relevant to whether the doc applies.
- Confirmed source content from the user, repository evidence, or existing docs.

## Procedure

Apply the Stop Conditions throughout; halt and report when any is met.

1. Identify the target document and the concern it owns per the convention. If the doc is
   not part of the project's tier, stop and report it.
2. Load the matching template and, in `revise` mode, the existing document.
3. Gather the confirmed inputs. Mark anything inferred as an assumption.
4. Write or update the document section by section. Keep content inside the doc's ownership
   boundary; when material belongs to another doc, place it there or leave a link, not a copy.
5. For `architecture.md`, apply the convention's split rule: when a section would bloat the
   overview, move detail to the named extension doc and leave a summary plus link behind.
6. Note whether `INDEX.md` needs to be re-synced as a result of this change. Do not edit
   `INDEX.md` here.

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
| Status | `completed`, `blocked`, or `skipped` |
| Document | File written or updated |
| Mode | `new` or `revise` |
| Sections | Sections created or changed |
| Ownership & links | Content moved or linked to other docs, or `none` |
| INDEX sync needed | `yes` or `no` |
| Assumptions | Inferences used, or `none` |
| Blockers | Unresolved issues, or `none` |
