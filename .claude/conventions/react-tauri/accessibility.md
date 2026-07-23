# Accessibility

Every WhisperPilot UI surface must be keyboard- and screen-reader-usable in the macOS
VoiceOver/WKWebView target. This convention defines implementation standards. For a changed
UI surface, the active TaskPilot item and its routed implementation artifact own its
interaction contract; if neither declares segment selection, treat transcript segments as
ordinary non-interactive reading content.

## Semantic controls and names

- Use native `<button>` elements for actions and `<a>` elements for navigation. Do not make a
  `<div>` or `<span>` interactive. A custom widget is permitted only when no native element
  can express the required interaction; it must use the applicable ARIA role, keyboard model,
  name, state, and focus behavior from the WAI-ARIA Authoring Practices.
- Give every interactive control an accessible name through visible text, an associated
  `<label>`, `aria-label`, or `aria-labelledby`. Use a visible `<label>` for text inputs and
  selects unless the surrounding UI already provides an equivalent programmatic label.
- Use native semantic elements and headings before adding ARIA. Do not add an ARIA role that
  changes the semantics of an equivalent native element.

## Keyboard and focus

- Every action available by pointer must be available from the keyboard. Native controls keep
  their native keyboard behavior; do not add duplicate Space or Enter handlers to them.
- Preserve a visible focus indicator with at least a 3:1 contrast ratio against adjacent
  colors. Do not remove `:focus-visible` styling unless an equivalent indicator replaces it.
- Keep focus order aligned with visual and task order. On opening or closing a modal, move
  focus into the modal and return it to the invoking control; trap focus while the modal is
  open.

## Status, transcript, and color

- Expose loading, completion, and error states as text, not color alone. Use `role="alert"`
  for errors that require immediate attention; use a polite live region for non-urgent status
  changes.
- Do not put the entire, continuously updated transcript in a live region. Announce only a
  concise, user-relevant update when the active interaction contract requires it; set
  `aria-atomic="true"` on that dedicated status region.
- A transcript segment is keyboard-focusable only when it is actionable or selectable. When
  segments are selectable, implement a documented keyboard model and expose the selected
  state programmatically; ordinary reading and scrolling retain native browser behavior.
- Text and meaningful non-text UI must meet WCAG 2.1 AA contrast: 4.5:1 for normal text,
  3:1 for large text, and 3:1 for controls, icons, and focus indicators.

## Review checklist

- [ ] Native controls are used wherever possible; any custom widget has its complete ARIA and keyboard model
- [ ] All controls have accessible names; inputs have programmatic labels
- [ ] Keyboard access and visible focus behavior work without duplicate native handlers
- [ ] Modal focus placement, trapping, and restoration are correct when a modal is changed
- [ ] Status and errors include text; live regions announce only concise relevant updates
- [ ] Transcript selection behavior is keyboard-accessible only when selection is supported
- [ ] Text and meaningful UI meet the stated contrast thresholds
