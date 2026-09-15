# WhisperPilot SDD document set

## Scope

The authoritative SDD root is \`docs/\`. WhisperPilot uses the Standard tier. TaskPilot owns requirements, tasks,
scenarios, delivery status, and \`WP-<n>\` identifiers. Documentation may cite those IDs but must not duplicate their
lifecycle or acceptance records.

## Standard documents

- \`INDEX.md\`: document map and ADR log.
- \`idea.md\`: product scope, principles, users, and non-goals.
- \`architecture.md\`: technical boundaries, components, data flow, security, and runtime ownership.
- \`design.md\`: UX flows, screens, states, interaction rules, and product copy.
- \`testing.md\`: test strategy, environments, quality policy, and known limitations.
- \`roadmap.md\`: milestones, sequencing, and delivery dependencies.
- \`decisions/ADR-NNN-<slug>.md\`: durable decision context, choice, and consequences.

Link to an owning document instead of repeating its facts elsewhere.

## Extension documents

Use \`api.md\`, \`db.md\`, \`security.md\`, \`operations.md\`, \`integrations.md\`, \`glossary.md\`, or
\`designbook.md\` only when that concern no longer fits its main document. Create an extension for a present need, not
to complete a template set.

An extension remains linked from its parent document and registered in \`docs/INDEX.md\`. \`architecture.md\` keeps the
system overview after technical detail moves to an extension.

## Index rules

\`docs/INDEX.md\` registers every root Markdown document except itself, every root subfolder containing Markdown, and
every \`decisions/ADR-*\` file.

Generated regions use these exact markers:

\`\`\`text
<!-- sdd-index-sync:begin documents -->
<!-- sdd-index-sync:end documents -->
<!-- sdd-index-sync:begin decisions -->
<!-- sdd-index-sync:end decisions -->

\`\`\`

Synchronization owns row presence and key cells. Preserve retained curated cells, row order, and all text outside the
markers. Append new keys with \`TODO\` in curated cells. Read each ADR status from its file; do not restrict status to a
fixed allowlist.

## Source rules

Use repository evidence or confirmed user input. Do not invent facts to fill a template. Use \`N/A\` only when a
template explicitly permits it. Unresolved conflicts block the document change.
