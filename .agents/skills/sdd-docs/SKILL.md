---
name: sdd-docs
description: Create or revise one WhisperPilot SDD document and keep docs/INDEX.md structurally synchronized.
---

# SDD documents

## Apply when

Use for `docs/idea.md`, `architecture.md`, `design.md`, `testing.md`, `roadmap.md`, an ADR, an extension document, or
`docs/INDEX.md`.

## Sources

- Ownership and tier: `.claude/conventions/sdd-doc-set.md`.
- Templates: `.claude/sdd/templates/docs/`.
- Existing `docs/` content and verified repository evidence.

## Policy

- WhisperPilot uses the Standard SDD tier.
- Edit one owning document per invocation; link to facts owned elsewhere.
- Do not invent project facts or copy TaskPilot requirements into `docs/`.
- Preserve existing structure and curated content unless the request changes it.
- ADRs record durable decision rationale; TaskPilot records delivery scope and status.
- After any SDD change, update the document and decision rows inside the marked `docs/INDEX.md` regions.
- Preserve retained index cells and all text outside the markers.
- New index rows use `TODO` for human-curated cells; removed files remove their rows.
- Never infer ADR status from an allowlist; read the status token from the ADR.

## Review

Use `sdd-auditor` for an independent completeness or docs-versus-code audit when fresh context is useful.

## Result

Report the document, changed sections, verified sources, index rows changed, and unresolved assumptions.
