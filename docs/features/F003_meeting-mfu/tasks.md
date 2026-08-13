# F003 Structured Meeting MFU — Tasks

Not yet scheduled in TaskPilot (M2 — library + speaker roles — is the active
backlog). TaskPilot items will be created when M3 is picked up; the TaskPilot
column is `—` until then.

| ID | Task | Implements | Depends on | TaskPilot |
| --- | --- | --- | --- | --- |
| F003-T1 | Add llama.cpp binding + Qwen2.5 model asset (download/verify) | F003-R1, F003-R6 | — | — (unscheduled) |
| F003-T2 | `generate_mfu`: transcript → five-section structured MFU via local LLM on Metal, driven by Create MFU (UI-blocking, spinner+timer) | F003-R1, F003-R2, F003-R6 | F003-T1 | — (unscheduled) |
| F003-T3 | MFU section UI: Create MFU trigger, render five sections, edit in place (auto-saved), copy, clear | F003-R2, F003-R3, F003-R5 | F003-T2 | — (unscheduled) |
| F003-T4 | Regenerate with confirmation guarding manual edits; clear to empty | F003-R4 | F003-T3 | — (unscheduled) |

## MFU

- Largest asset the app manages (multi-GB GGUF); introduce only at M3.
- Prompt design for the five Russian sections (summary/decisions/action items/
  open questions/participants) is core to T2 and worth iterating on real
  transcripts; participants/action-item owners read best when F002 speaker labels
  are present.
- MFU persist on the meeting via F004 (auto-save).
