# F003 Summary / MFU — Tasks

Not yet scheduled in TaskPilot (M2 is the active backlog). TaskPilot items will
be created when M3 is picked up; the TaskPilot column is `—` until then.

| ID | Task | Implements | Depends on | TaskPilot |
| --- | --- | --- | --- | --- |
| F003-T1 | Add llama.cpp binding + Qwen2.5 model asset (download/verify) | F003-R1, F003-R5 | — | — (unscheduled) |
| F003-T2 | `summarize` command: transcript → summary via local LLM on Metal | F003-R1, F003-R5 | F003-T1 | — (unscheduled) |
| F003-T3 | Summary UI section: render, edit in place, copy to clipboard | F003-R2, F003-R3, F003-R4 | F003-T2 | — (unscheduled) |

## Notes

- Largest asset the app manages (multi-GB GGUF); introduce only at M3.
- Prompt design for Russian decisions/action-item extraction is part of T2 and
  worth iterating on real transcripts.
