# Change Hygiene

Mechanical checks that catch the defect classes which pass unit tests but break
in use. None of these require new tooling; they are short audits run during
implementation and enforced in review. Each maps to a real defect this project
has shipped and then had to fix in a later round.

## 1. State-lifecycle completeness

When you add a new piece of state (a `useState`/`useReducer` field, a Zustand
slice, a `Set`/`Map` of ids, a ref), enumerate **every** path that adds to it
**and every path that must remove from it** before moving on:

- add paths: create, submit, receive, select…
- removal paths: success, error, **delete**, **clear**, switch-away, cancel, unmount.

A new status set that is only cleared on the happy path leaks ids on delete or
clear. Write the add/remove paths as a short list (or a test) and confirm each
exit is handled.

## 2. Refactor-invariant re-check

When a change alters an assumption other code relied on, grep for the dependents
before declaring done:

- **Multiplicity change** (extracting a component so it can be rendered more than
  once): static DOM ids, `aria-describedby`/`htmlFor` links, singletons, and
  module-level state that were safe at one instance break at two. Derive ids
  with `useId()`; don't hard-code them in a shared component.
- **Constant change** (lowering a cap, renaming a token): grep for coupled
  constants and call sites — e.g. a word-break threshold that is now larger than
  the max length it was meant to sit under is dead logic.

## 3. Adversarial input coverage

For any validator, formatter, parser, or sanitizer, list the adversarial input
classes **before** implementing and cover them with tests:

- empty / whitespace-only
- boundary (exactly the max, max+1, exactly the min)
- wrong kind (disallowed scheme/char, leading punctuation, blank authority)
- over-length / truncation that must stay within its own bound
- inputs that satisfy the type but violate the intent (a title that is all
  punctuation; `https://` with no host)

Representative-input tests are not enough; a function that returns a value
violating its own documented invariant is a Major defect.

## 4. Integration re-audit after cumulative changes

**Trigger:** any task that changed more than one file, component, constant, or
token. Before closing, do a quick "what did this touch that I changed earlier in
this task?" pass: shared components, shared constants/tokens, shared state, and
shared CSS. Per-change review does not catch interaction defects between changes.

---

§1–§3 are advisory during implementation (run them yourself) and **enforced in
review**: `code-reviewer` loads this file and flags a stranded-state leak, a
broken invariant after a refactor/constant change, a missing adversarial test,
or an invariant-violating return. `code-reviewer.md` owns the severity it
assigns each (do not restate severities here, so the two never drift). §4 is
**advisory only** — apply judgment; it is not a standalone gated finding.
