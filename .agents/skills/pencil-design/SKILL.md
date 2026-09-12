---
name: pencil-design
description: Create, review, or synchronize WhisperPilot Pencil designs without editing .pen JSON directly.
---

# Pencil design

## Apply when

Use for pre-implementation UI design or for synchronization between `pencil/*.pen` and shipped UI. Do not use for a
code-only UI change with no design artifact in scope.

## Hard boundary

- Mutate `.pen` files only through the Pencil MCP or the supported `pen` CLI.
- Never edit, patch, format, or generate `.pen` JSON directly.
- If neither tool is available, stop without changing the design file.
- Keep design exploration separate from production implementation.

## Modes

- `design`: create or iterate a mockup from stated intent before implementation.
- `pen-to-code`: extract approved layout, tokens, states, copy, and assets as implementation facts.
- `code-to-pen`: update the design after final UI behavior and layout are known.

## Policy

- Read `docs/design.md` and the relevant current UI before changing an existing design.
- Preserve unrelated frames and components.
- Use named tokens and reusable components already present in the design.
- Render the affected frame after mutation.
- Use `visual-reviewer` with the render, intent, and optional counterpart screenshot.
- A design approval never authorizes production code changes.

## Result

Report mode, affected file and frames, tool used, render path, visual-review result, and unresolved drift.
