# ADR-006: llama.cpp + Qwen2.5 for local summarization

- **Status:** superseded for model selection by Phase 3 local-LLM qualification;
  the local llama.cpp engine decision remains accepted
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

M3 generates a short summary / MFU from the transcript. The requirement is a
**local** LLM (no cloud), with strong Russian summarization, running on Apple
Silicon.

## Decision

Run a quantized **Qwen2.5-Instruct** model via **llama.cpp** on Metal for
summarization. Fully on-device; the transcript never leaves the machine.

As of 2026-09-12, Qwen2.5 has been removed from the active catalog. The same
local llama.cpp architecture now offers the qualified Qwen3.5 Fast, Qwen3.8
Quality, and Gemma 4 Quality profiles recorded in
`docs/validation/phase-3-llm-qualification.md`.

## Consequences

- Meets the local-first requirement; Qwen2.5 summarizes Russian well and runs
  comfortably on the target hardware via Metal.
- Adds a new dependency (llama.cpp bindings + a multi-GB model download) — the
  largest asset the app manages; introduced only at M3.
- Summaries are editable before use, so occasional model weakness is correctable
  by the user.

## Alternatives Considered

- **Reuse a cloud LLM (e.g. OpenAI)** — best quality and fastest to wire, but
  sends meeting content off-device, contradicting the local-first principle.
- **A smaller local model** — lighter, but weaker Russian summarization; Qwen2.5
  is the quality/size sweet spot for the target machine.
