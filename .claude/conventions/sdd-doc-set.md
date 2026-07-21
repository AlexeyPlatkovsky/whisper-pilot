# SDD Doc Set

## Purpose

Define the canonical Spec-Driven Development document set: the folder layout, what each
document owns, the feature-folder schema, the identifier scheme, the tiers, and the
traceability spine that links intent down to verification.

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
  features/F<NNN>_<short-name>/{requirements.md,tasks.md,scenarios.md}
```

The authoritative documentation root is `docs/`. If a project already keeps authoritative
docs elsewhere, preserve that root and record it in `INDEX.md` rather than relocating files.

## WhisperPilot Specifics

- The authoritative docs root is `docs/` at the repository root. It already contains
  `idea.md` (product specification) and `architecture.md` (technical architecture).
  Reconcile with these files; do not recreate them.
- The project adopts the **Standard** tier: `idea`, `architecture`, `design`, `testing`,
  `roadmap`, `INDEX`, `decisions/`, and `features/`.
- There is no design-system extension doc yet; UI design detail lives in `design.md`.
  Add a `design-book.md` extension only if the design system grows enough to warrant it.
- Templates for this doc set live under `.claude/sdd/templates/`.
- TaskPilot (`WP-<n>` IDs) is the sole tracker of work status per `AGENTS.md`. A feature's
  `tasks.md` records the spec-side breakdown and traceability; when a TaskPilot item exists
  for a task, reference its `WP-<n>` ID in the task row rather than duplicating status flow.

## Document Ownership

Each document owns one concern. Do not duplicate a concern across documents; link instead.

| Document | Owns | Does not own |
| --- | --- | --- |
| `idea.md` | Problem, users/personas, value, in/out scope, non-goals, principles, success signals | Technical structure, UX detail |
| `architecture.md` | System context, components, data model, tech stack, integrations, constraints, cross-cutting concerns | Product/UX flows, decisions log |
| `design.md` | Product/UX design: user flows, key screens and states (empty/loading/error), interaction patterns, UX principles, accessibility | Technical components, code structure |
| `testing.md` | Test strategy: levels, tooling, environments, coverage expectations, quality gates, how feature `scenarios.md` and checklists are executed | Per-feature scenario content |
| `roadmap.md` | Phases, milestones, release stance, sequencing, dependencies, non-goals over time | Per-feature task breakdown |
| `decisions/` | One ADR per significant decision: context, decision, status, consequences, alternatives | Behavioral rules |
| `INDEX.md` | Live map of all docs + the feature registry and traceability links | Any authority or behavioral rule |

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

## Feature Folder Schema

Each feature is a folder `features/F<NNN>_<short-name>/` containing exactly:

- `requirements.md` — feature summary, links up to the `idea.md`/`roadmap.md` item it
  serves, functional requirements (each with an ID), acceptance criteria, constraints,
  explicit out-of-scope.
- `tasks.md` — task breakdown; each task has an ID, dependencies, a link to the
  requirement ID(s) it implements, and its TaskPilot ID. TaskPilot owns task status.
- `scenarios.md` — behavior verification: Gherkin `Given/When/Then` scenarios plus a manual
  verification checklist; each scenario links to the requirement ID(s) it covers.

## Identifier Scheme

- Feature: `F<NNN>` with a zero-padded sequential number, e.g. `F001`. Folder name is
  `F<NNN>_<short-name>` where `<short-name>` is a kebab-case slug.
- Requirement: `F<NNN>-R<n>`, e.g. `F001-R1`.
- Task: `F<NNN>-T<n>`, e.g. `F001-T1`.
- Scenario: `F<NNN>-S<n>`, e.g. `F001-S1`.
- Decision: `ADR-<NNN>`, zero-padded sequential, e.g. `ADR-001`.

IDs are stable once assigned. Do not renumber existing IDs; mark superseded items instead.

## Tiers

A project adopts one tier; tiers are additive supersets.

- **Lean** — `idea.md`, `architecture.md`, `roadmap.md`, `INDEX.md`. No `features/`.
- **Standard** (default) — Lean + `design.md`, `testing.md`, `decisions/`, and `features/`.
- **Full** — Standard with full per-feature requirement IDs, scenarios for every
  requirement, and an ADR for every significant decision.

Omit documents a tier does not include rather than shipping empty placeholders.

## Traceability Spine

Intent flows down and verification links back up:

```
idea.md
  └─ roadmap.md (phase/milestone)
       └─ features/F<NNN>/requirements.md (F<NNN>-R<n>)
            ├─ tasks.md          (F<NNN>-T<n>  → F<NNN>-R<n>)
            └─ scenarios.md      (F<NNN>-S<n>  → F<NNN>-R<n>)
architecture.md / design.md constrain feature requirements
decisions/ADR-<NNN> records why a constraint or direction was chosen
```

Every requirement should trace up to an `idea.md` scope item or `roadmap.md` entry, and
down to at least one task and one scenario. `INDEX.md` records the current state of these
links. A requirement with no scenario, or a scenario with no requirement, is a traceability
gap that review must flag.
