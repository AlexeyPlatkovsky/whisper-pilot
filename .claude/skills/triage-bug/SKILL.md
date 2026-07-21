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
- **Environment:** OS, build mode (dev/release), relevant CLI tool version if applicable
- **Frequency:** always / intermittent / once observed

If `Steps to reproduce` is `unknown`, proceed to Phase 2 with that noted — reproduction attempts may clarify it.

If the report describes two or more distinct defects (compound report), stop and surface this to the user: state which distinct issues are visible and ask whether to triage them separately or as one report before proceeding.

---

### Phase 2 — Reproduce

Attempt to reproduce the bug in the local environment.

#### 2a — Check Logs First

Before running any commands, check whether log output from a prior run is already available. Log files are written to `~/Library/Logs/WhisperPilot/voicepilot-YYYY-MM-DD.log`.

```bash
ls ~/Library/Logs/WhisperPilot/
tail -200 ~/Library/Logs/WhisperPilot/voicepilot-$(date +%Y-%m-%d).log
```

If the 200-line tail does not reach the relevant component, search by pattern instead:

```bash
grep -n "resampler:\|feed_audio:\|whisper:\|vad:\|segment " ~/Library/Logs/WhisperPilot/voicepilot-$(date +%Y-%m-%d).log | tail -100
```

If `ls ~/Library/Logs/WhisperPilot/` returns empty or the directory does not exist, record `Log evidence: absent`. For non-front-end-only bugs, skip to RUST_LOG=debug collection below; for confirmed front-end-only bugs (UI rendering, visual copy, test tooling), proceed directly to Phase 2b.

**Key log patterns** (all operational metadata — no audio samples, transcript text, or API keys):
- `resampler:` — sample counts in/buffered/out; zeros indicate pipeline stalls
- `feed_audio:` — 48 kHz in, 16 kHz out, rolling window size
- `whisper: transcribing window=` — samples Whisper receives (expect ~16 000–80 000 for 1–5 s)
- `whisper: hallucination filtered` — noise artefacts discarded by the filter
- `vad: silence boundary` — VAD-triggered speech-end flush
- `segment finalized:` — text_len and timestamp; absence means no segments reached the adapter

**When to collect RUST_LOG=debug output:** ask the user to run with debug logging when the bug is not confirmed front-end-only (i.e., not isolated to UI rendering, visual copy, or test tooling). This covers all audio, transcription, pipeline, VAD, session, IPC, and adapter bugs.

```bash
RUST_LOG=debug npm run tauri dev 2>&1 | tee /tmp/vp-debug.log
```

If neither the tail nor the grep returns any of the six key patterns, record `Log evidence: partial — file exists, no key patterns found` and note which patterns were searched. For non-front-end-only bugs, ask the user to collect RUST_LOG=debug output.

Record relevant log excerpts in the `Observed output` field prefixed with `[from log]`. If no log is available, record `Log evidence: absent` and note that debug output must be collected before the next reproduction attempt.

#### 2b — Run Checks

Commands depend on the affected layer:

```
npm run dev          # start front-end dev build (if UI behavior is reported)
cargo build          # verify Rust core compiles
cargo nextest run    # check for related failures in existing tests
```

Record:
- **Reproduced:** yes / no / intermittent
- **Reproduction steps used:** exact commands or UI actions
- **Observed output:** exact error, panic, wrong UI state, test failure output, or relevant log lines

Make at most three reproduction attempts. If not reproduced after three runs, record `Reproduced: no / intermittent` with the evidence, set root cause to `unknown — not reproducible`, and skip to Phase 4.

---

### Phase 3 — Root Cause

Identify the specific code location and mechanism causing the bug. Use `Read` and `Bash` (grep, cargo check, test output) to locate the defect.

If the bug involves existing UI flow, IPC routing, Rust core process/adapters, or storage behavior, read the relevant sections of `docs/architecture.md` before concluding root cause. If root-cause evidence crosses layers, load the additional relevant sections. Skip architecture docs when the bug is isolated to tests, copy, or unrelated tooling, and record the skip reason in the rationale if relevant.

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

**Type:** logic-error / UI-regression / IPC-error / subprocess-error / state-management / test-only / other (specify)

**Scope:** front-end / Rust core / both

---

### Phase 5 — Dispose

Choose exactly one disposition:

| Disposition | Criteria |
|---|---|
| `fix-bug` | Reproduced and root cause known — route to `fix-bug` pipeline |
| `needs-more-info` | Cannot reproduce or root cause is unknown — return to user with specific questions |
| `known-issue` | Already tracked; record the existing reference and close |
| `wont-fix` | Out of scope, by design, or cost exceeds value — document reasoning and close |

---

## Output Contract

After completing all phases, emit:

`Skill: triage-bug - output below`

**Bug Summary**
- Description:
- Reproduced: yes / no / intermittent
- Log evidence: present / partial / absent (note if key patterns were found)
- Root Cause: (location + mechanism, or `unknown`)
- Confidence: high / medium / low

**Classification**
- Severity: Critical / High / Medium / Low
- Type:
- Scope: front-end / Rust core / both

**Disposition:** fix-bug / needs-more-info / known-issue / wont-fix

**Rationale:** one-to-three sentences explaining the disposition choice.

**Next Step:** if disposition is `fix-bug`, state "Route to `fix-bug` pipeline. Preconditions are satisfied." Otherwise state the blocking question or closure reason.
