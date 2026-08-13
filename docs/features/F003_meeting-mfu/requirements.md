# F003 Structured Meeting MFU — Requirements

## Summary

Generate **structured meeting MFU** (the MFU section) from the transcript using
a local LLM, on demand via the **Create MFU** action, shown below the transcript,
editable in place (auto-saved), clearable, and copyable. In Russian. Sections:
summary, key decisions, action items, open questions, participants. Fully
on-device.

## Serves

- `idea.md` scope: "structured meeting MFU generated locally … editable and
  copyable".
- `roadmap.md` phase: **M3 — Structured meeting MFU**.

## Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| F003-R1 | The system shall generate structured meeting MFU from the transcript using a local LLM (llama.cpp / Qwen2.5), in Russian, with sections: summary, key decisions, action items (owner + task), open questions, participants. | must |
| F003-R2 | The system shall generate MFU on demand via the **Create MFU** action (enabled only after transcription finishes) and present them in the MFU section below the transcript; generation blocks the UI with a spinner + live timer and is not cancellable. | must |
| F003-R3 | The system shall let the user edit any MFU section in place, auto-saved to the meeting. | must |
| F003-R4 | The system shall let the user regenerate the MFU on demand, with confirmation before replacing generated content, and clear the MFU (returning the MFU section to empty). | should |
| F003-R5 | The system shall copy the MFU to the clipboard on request. | must |
| F003-R6 | The system shall perform detail generation entirely locally, with no network access. | must |

## Acceptance Criteria

- **F003-R1:** a transcript yields MFU with all five sections; action items
  carry an owner and a task; participants reflect the speakers present.
- **F003-R2:** Create MFU is disabled until transcription finishes; pressing it
  blocks the UI with a spinner + live timer and renders the MFU below the
  transcript when done.
- **F003-R3:** an edited section persists in the meeting (auto-saved) and
  survives reopen.
- **F003-R4:** regenerate re-produces the MFU after confirmation; manually
  edited content is not silently lost without confirmation; clear empties the
  MFU section back to its default (15%) state.
- **F003-R5:** copy places the current (edited) MFU on the clipboard.
- **F003-R6:** no network request is made during generation.

## Constraints

- Local llama.cpp + quantized Qwen2.5-Instruct on Metal (ADR-006, ADR-009); model
  managed like other assets.
- MFU are stored on the meeting (F004 persistence) and auto-saved.
- Reads better with speaker labels present (F002) but does not require them
  (`roadmap.md` sequencing).

## Out of Scope

- Cloud summarization or any network fallback.
- User-configurable section sets or templates (fixed five-section set for now).
