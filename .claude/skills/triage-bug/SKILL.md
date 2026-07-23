---
name: triage-bug
description: Investigates a reported bug in WhisperPilot — gathers available information, attempts reproduction, identifies root cause, classifies severity and type, and decides disposition. Produces a triage report; writes no production code.
---

# Skill: triage-bug

## When This Skill Applies

Use when a bug has been reported (user description, failing test output, error log, or unexpected behavior) and the goal is to understand, classify, and dispose of it before any fix begins.

Do not use when:
- The root cause is already known and confirmed — route directly to `fix-bug` pipeline.
- The task is to implement new behavior or refactor working code.
- The report is a design disagreement rather than a defect.

## Core Instructions

This skill does not write production code. Its only output is a triage report.

---

### Phase 1 — Gather

Collect all available information. Do not infer; record only what is stated or directly observable. Mark any field `unknown` if not available.

Required fields:
- **Description:** what is wrong
- **Steps to reproduce:** exact user or system actions that trigger it
- **Expected behavior:** what should happen
- **Actual behavior:** what actually happens (include error messages verbatim)
- **Environment:** macOS version, build mode (dev/release), source media format, selected language/model, and model-download state when applicable
- **Frequency:** always / intermittent / once observed

If `Steps to reproduce` is `unknown`, proceed to Phase 2 with that noted — reproduction attempts may clarify it.

If the report describes two or more distinct defects (compound report), stop and surface this to the user: state which distinct issues are visible and ask whether to triage them separately or as one report before proceeding.

---

### Phase 2 — Reproduce

Attempt to reproduce the bug in the local environment.

#### 2a — Gather Diagnostic Evidence

The current application initializes `env_logger` but does not define a persistent log-file
contract. Do not assume a `~/Library/Logs` path or inspect a user's files without a supplied
path. Use one of these sources, in order: the reported error, failing test output, a console
excerpt supplied by the user, or a scoped local reproduction with `RUST_LOG=debug`.

For local media-processing bugs, ask for or record only operational evidence: media format,
model readiness, error text, command output, and whether diarization fell back to
speaker-less segments. Do not request audio contents, full transcript contents, or unrelated
personal files.

```bash
RUST_LOG=debug npm run tauri -- dev
```

Run this only when a local reproduction is authorized and safe. Record diagnostic excerpts in
`Observed output` prefixed with `[console]`; otherwise record `Diagnostic evidence: absent`.

#### 2b — Run Checks

Commands depend on the affected layer:

```
npm run tauri -- dev                                   # real macOS Tauri window, if UI behavior is reported
npm run test:run                                       # relevant front-end tests
cargo test --manifest-path src-tauri/Cargo.toml        # relevant Rust tests
```

Record:
- **Reproduced:** yes / no / intermittent
- **Reproduction steps used:** exact commands or UI actions
- **Observed output:** exact error, panic, wrong UI state, test failure output, or relevant log lines

Define one attempt as one complete execution of the recorded command or UI
action sequence. Make at most three attempts. Zero successes is `no`; at least
one success and one failure is `intermittent`; all attempted runs reproducing is
`yes`. For `no`, set root cause to `unknown — not reproducible` and skip to
Phase 4.

---

### Phase 3 — Root Cause

Identify the specific code location and mechanism causing the bug. Use `Read` and `Bash` (grep, cargo check, test output) to locate the defect.

If the bug involves existing UI flow, IPC routing, Rust core processing, model management, settings, or meeting storage, read the relevant sections of `docs/architecture.md` before concluding root cause. If root-cause evidence crosses layers, load the additional relevant sections. Skip architecture docs when the bug is isolated to tests, copy, or unrelated tooling, and record the skip reason in the rationale if relevant.

Record:
- **Location:** file path and line range (or `unknown` if not locatable)
- **Mechanism:** one-to-three-sentence explanation of why the bug occurs
- **Confidence:** high / medium / low, with reason if not high
- **Related code:** any other files or functions that may be affected

Do not make speculative claims beyond what the code and reproduction evidence support. If the defect location cannot be found after examining the reproduction output and up to five targeted searches, record `Location: unknown` with the searches attempted and proceed to Phase 4.

---

### Phase 4 — Classify

**Severity:**

| Level | Criteria |
|---|---|
| Critical | Data loss, security issue, crash on the happy path, or complete feature failure |
| High | Core feature broken for most users; no workaround |
| Medium | Feature partially broken or degraded; workaround exists |
| Low | Minor UI glitch, edge-case failure, or cosmetic issue |

**Type:** logic-error / UI-regression / IPC-error / media-processing / model-management / storage-error / state-management / test-only / other (specify)

**Scope:** front-end / Rust core / both / tooling-build-CI / N/A

---

### Phase 5 — Dispose

Choose exactly one disposition:

| Disposition | Criteria |
|---|---|
| `fix-bug` | Reproduced and root cause known — route to `fix-bug` pipeline |
| `needs-more-info` | Cannot reproduce or root cause is unknown — return to user with specific questions |
| `known-issue` | A verified existing TaskPilot ID covers it; recommend linking and route mutation through `taskpilot-work` |
| `recommend-wont-fix` | Evidence shows out-of-scope or by-design behavior; require explicit user/product-owner disposition |

---

## Output Contract

After completing all phases, emit:

`Skill: triage-bug - output below`

**Bug Summary**
- Description:
- Steps to reproduce:
- Expected behavior:
- Actual behavior:
- Environment:
- Frequency:
- Reproduced: yes / no / intermittent
- Reproduction attempts and observed output:
- Diagnostic evidence: present / partial / absent (state source, minimal redacted excerpt, and whether user media content was avoided)
- Root Cause: (location + mechanism, or `unknown`)
- Confidence: high / medium / low
- Related code:

**Classification**
- Severity: Critical / High / Medium / Low
- Type:
- Scope: front-end / Rust core / both / tooling-build-CI / N/A

**Disposition:** fix-bug / needs-more-info / known-issue / recommend-wont-fix

**Rationale:** one-to-three sentences explaining the disposition choice.

**Next Step:** for `fix-bug`, state “Triage-specific reproduction and
root-cause prerequisites are satisfied; return to the manager for TaskPilot,
Git, and pipeline gates.” Otherwise state the exact question, verified existing
ID, or approval required.
