# SDD Doc Set

## Purpose

Define the canonical Spec-Driven Development document set: the folder layout, what each
document owns, the identifier scheme, the tiers, and the traceability spine that links
intent down to verification.

This convention is factual and structural. It defines what the doc set *is*, not how an
agent creates or reviews it. Creation lives in the SDD skills; sequencing lives in the SDD
pipelines.

## Folder Layout

```
docs/
  INDEX.md
  idea.md
  architecture.md
  design.md
  testing.md
  roadmap.md
  <extension docs, optional: api.md, db.md, security.md, operations.md, ...>
  decisions/ADR-NNN-<slug>.md
```

The authoritative documentation root is `docs/`. If a project already keeps authoritative
docs elsewhere, preserve that root and record it in `INDEX.md` rather than relocating files.

Requirements, tasks, and scenarios are not `docs/` artifacts: they are tracked as
TaskPilot items (project key **WP**), and their identifiers and traceability live there,
not under `docs/`.

## WhisperPilot Specifics

- The authoritative docs root is `docs/` at the repository root. It already contains
  `idea.md` (product specification) and `architecture.md` (technical architecture).
  Reconcile with these files; do not recreate them.
- The project adopts the **Standard** tier: `idea`, `architecture`, `design`, `testing`,
  `roadmap`, `INDEX`, and `decisions/`.
- There is no design-system extension doc yet; UI design detail lives in `design.md`.
  Add a `designbook.md` extension only if the design system grows enough to warrant it.
- Templates for this doc set live under `.claude/sdd/templates/`.
- TaskPilot (`WP-<n>` IDs) is the sole tracker of features, requirements, tasks, scenarios,
  and their status per `AGENTS.md`. Nothing in `docs/` duplicates that tracking; a document
  that needs to reference a piece of tracked work cites its `WP-<n>` ID.

## Document Ownership

Each document owns one concern. Do not duplicate a concern across documents; link instead.

| Document | Owns | Does not own |
| --- | --- | --- |
| `idea.md` | Problem, users/personas, value, in/out scope, non-goals, principles, success signals | Technical structure, UX detail |
| `architecture.md` | System context, components, data model, tech stack, integrations, constraints, cross-cutting concerns | Product/UX flows, decisions log |
| `design.md` | Product/UX design: user flows, key screens and states (empty/loading/error), interaction patterns, UX principles, accessibility | Technical components, code structure |
| `testing.md` | Test strategy: levels, tooling, environments, coverage expectations, quality gates, how TaskPilot scenarios and checklists are executed | Per-feature scenario content (lives in TaskPilot) |
| `roadmap.md` | Phases, milestones, release stance, sequencing, dependencies, non-goals over time | Per-feature task breakdown (lives in TaskPilot) |
| `decisions/` | One ADR per significant decision: context, decision, status, consequences, alternatives | Behavioral rules |
| `INDEX.md` | Live map of all docs and the decision log | Any authority or behavioral rule; feature, requirement, task, or scenario tracking |

`INDEX.md` is a lookup aid only. It must not contain routing, gates, or behavioral rules.

## Optional Extension Docs

`architecture.md` is the always-present technical overview. When a topic would bloat it,
move the detail into an extension doc and leave a one-paragraph summary plus a link in
`architecture.md`. Extension docs are optional, tier-independent, and added only when
warranted.

Use this recognized vocabulary so names stay consistent across projects:

| Doc | Owns |
| --- | --- |
| `api.md` | API / interface contracts |
| `db.md` | Persistence: data model, schema, migrations |
| `security.md` | Threat model, authn/authz, secrets handling |
| `operations.md` | Deployment, runtime, observability, runbooks |
| `integrations.md` | External service contracts and dependencies |
| `glossary.md` | Domain vocabulary |
| `designbook.md` | Design tokens, themes, and component patterns |

Add a doc outside this list only when none fits; record it in `INDEX.md` so it is discoverable.

### When to split

Split a section out of `architecture.md` when any of these hold:

- it is routinely consulted on its own,
- it has its own audience or lifecycle, or
- it has grown large enough to hurt selective loading (context pollution).

Splitting always leaves a summary + link behind in `architecture.md` and a registry row in
`INDEX.md`. The same rule applies to any main doc, e.g. `design.md`.

### Placement and escalation

Extension docs default to flat files at the `docs/` root (`docs/api.md`). When one topic
grows into a family — for example several API areas or subsystems — promote it to a
subfolder `docs/<domain>/` with its own mini-index, and link to that index from
`architecture.md` and `INDEX.md`. Do not create subfolders pre-emptively.

## Identifier Scheme

- Decision: `ADR-<NNN>`, zero-padded sequential, e.g. `ADR-001`.
- Feature, requirement, task, and scenario identifiers are TaskPilot item IDs
  (`WP-<n>`), assigned and owned by TaskPilot; `docs/` neither assigns nor mirrors them.

IDs are stable once assigned. Do not renumber existing IDs; mark superseded items instead.

## Tiers

A project adopts one tier; tiers are additive supersets.

- **Lean** — `idea.md`, `architecture.md`, `roadmap.md`, `INDEX.md`.
- **Standard** (default) — Lean + `design.md`, `testing.md`, and `decisions/`.
- **Full** — Standard, plus: a decision recorded in `architecture.md` or that
  reverses a prior ADR carries its own ADR, and every `sdd-doc-author` edit is
  followed by an `sdd-spec-reviewer` pass before the routing TaskPilot item
  closes. Standard runs that reviewer only when adopting or restructuring the
  tree, so the reviewer's cadence is what separates the two tiers, not the
  document list.

Omit documents a tier does not include rather than shipping empty placeholders.

## Traceability Spine

Intent flows down and verification links back up:

```
idea.md
  └─ roadmap.md (phase/milestone)
       └─ TaskPilot item (WP-<n>: epic/feature/task, with its scenarios)
architecture.md / design.md constrain what a TaskPilot item may implement
decisions/ADR-<NNN> records why a constraint or direction was chosen
```

Every TaskPilot item should trace up to an `idea.md` scope item or `roadmap.md` entry.
`docs/` does not track or index individual TaskPilot items; TaskPilot is the record of
that traceability.

No gate in this system currently enforces that upward link: `sdd-spec-reviewer`
reviews `docs/` only and does not read TaskPilot, and no skill or agent audits
items for it. An item that traces to nothing therefore passes unnoticed unless a
person catches it. Treat this as a known unenforced convention — the scope item
or milestone an item serves should be discoverable from the item
(`taskpilot-work` owns its fields), and stated plainly as absent when none
exists, rather than assuming a later gate will ask.
