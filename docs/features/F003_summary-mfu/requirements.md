# F003 Summary / MFU — Requirements

## Summary

Generate a short summary / MFU (decisions and action items) from the finalized
transcript using a local LLM, shown below the transcript, editable in place and
copyable to the clipboard. Fully on-device.

## Serves

- `idea.md` scope: "a short summary / MFU generated locally, editable and
  copyable".
- `roadmap.md` phase: **M3 — Summary / MFU**.

## Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| F003-R1 | The system shall generate a short summary / MFU from the transcript using a local LLM (llama.cpp running Qwen2.5). | must |
| F003-R2 | The system shall present the summary in a section below the transcript. | must |
| F003-R3 | The system shall let the user edit the summary in place. | must |
| F003-R4 | The system shall copy the summary to the clipboard on request. | must |
| F003-R5 | The system shall perform summarization entirely locally, with no network access. | must |

## Acceptance Criteria

- **F003-R1:** a transcript yields a concise summary capturing its decisions and
  action items; the model asset is downloaded and verified before use.
- **F003-R2:** the summary section appears under the transcript once available.
- **F003-R3:** edits to the summary persist in the UI and in any subsequent copy.
- **F003-R4:** the copy action places the current summary text on the clipboard.
- **F003-R5:** no network request is made during summarization.

## Constraints

- Local llama.cpp + quantized Qwen2.5-Instruct on Metal (ADR-006); model managed
  like other assets.
- Summarization consumes the finalized transcript; independent of F002 but reads
  better with speaker labels present (`roadmap.md` sequencing).
- Summary is editable before use, tolerating occasional model weakness.

## Out of Scope

- Cloud summarization or any network fallback.
- Templated/structured MFU formats beyond a short free-form summary (future).
