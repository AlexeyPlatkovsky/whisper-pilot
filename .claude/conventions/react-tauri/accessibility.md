# Accessibility

The transcription panel is a keyboard- and screen-reader-usable surface on macOS (VoiceOver).

## Hard rules
- Interactive elements are real controls: `<button>` for actions, `<a>` for links. Don't put click handlers on `<div>`/`<span>` without `role` + keyboard handling.
- Every control has an accessible name (visible label, `aria-label`, or `aria-labelledby`).
- Inputs have associated `<label>`s.
- Focus order is logical; focus is visible (don't remove focus outlines without an equivalent).
- Transcription controls work from the keyboard (Space to toggle, Tab to navigate), not mouse-only.

## Transcription specifics
- New transcript segments should be announced — use an `aria-live="polite"` region on the transcript container so screen-reader users hear new text.
- Loading and error states are conveyed as text/ARIA, not color alone.
- Provide keyboard navigation through the transcript history (Arrow keys to scroll, focus management for selectable segments).
- Color contrast meets WCAG AA for message text on the bubble/panel background.

## Review checklist
- [ ] No interactive `div`/`span` without role + key handlers
- [ ] All controls have accessible names
- [ ] Inputs have labels
- [ ] Visible focus indicator preserved
- [ ] New transcript segments announced via `aria-live="polite"` region
- [ ] State (loading/error) not conveyed by color alone
- [ ] Transcript history keyboard-navigable
