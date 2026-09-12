# Phase 3 local-LLM qualification

Date: 2026-09-12. Corpus revision: 1 in
`src-tauri/tests/fixtures/llm_profile_corpus.json`.

## Acceptance matrix

All candidates must load through WhisperPilot's pinned `llama-cpp-2` Metal
runtime, return all five MFU fields, translate RU→EN and EN→RU, conservatively
polish mixed Recorder text, and complete the repeated long mixed MFU input.
No accepted output may expose reasoning/control tokens, change protected
numbers or identifiers, lose the dominant target language, or exceed the
application's length guards. Malformed JSON must remain an explicit failure.
Fast targets interactive latency and 12 GB minimum unified memory. Quality is
opt-in, targets better instruction fidelity, and requires 24 GB minimum.

## Environment

- Apple M5 Pro, 48 GB unified memory, macOS 26.6.2.
- WhisperPilot `llama-cpp-2` runtime with Metal and 16,384-token application
  context; external inspection used llama.cpp build 10621 (`c1d0e7a00`).
- Tests ran locally with no transcript upload. Fixtures are synthetic.

## Pinned assets

| Profile | Revision and file | Bytes | SHA-256 | Terms |
| --- | --- | ---: | --- | --- |
| Fast | `unsloth/Qwen3.5-4B-GGUF@e87f176479d0855a907a41277aca2f8ee7a09523`, `Qwen3.5-4B-Q4_K_M.gguf` | 2,740,937,888 | `00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4` | Apache-2.0 upstream |
| Quality | `empero-ai/Qwen3.8-9B-Distill-GGUF@760121cd70bb4c36b2b5ec58eb765e0df5987efe`, `Qwen3.8-9B-Q6_K.gguf` | 7,558,901,056 | `0f1271373f899912bfe4ea76299af7dd83722d98ea421b0827501c3a2c6da22b` | Apache-2.0 upstream |
| Quality | `google/gemma-4-12B-it-qat-q4_0-gguf@29d097773436b69ff9feafd636ab4cf873786537`, `gemma-4-12b-it-qat-q4_0.gguf` | 6,975,879,296 | `93567e57a8fe10b23569b9d9ec38cd005deedf71e29477c421a4b83f418a538b` | Gemma Terms; redistribution must carry applicable terms and notices |

## Real-Metal results

The reproducible command is `scripts/benchmark-llm-profiles.sh` with the three
verified local files. The full run includes RU/EN translation, mixed Recorder
polish, short mixed MFU, and a 40-times repeated mixed MFU input.

| Candidate | Full corpus wall time | Representative accepted output | Result |
| --- | ---: | --- | --- |
| Qwen3.5 4B Q4_K_M | 10.68 s | `Сэм опубликует сборку WP-129 в 15:30 с бюджетом 42 доллара США.` | Go; fresh-install Fast recommendation |
| Qwen3.8 9B Q6_K | 15.88 s | `Сэм опубликует сборку WP-129 в 15:30 с бюджетом 42 USD.` | Go; Russian, English, and code-switching gates passed |
| Gemma 4 12B QAT Q4_0 | 17.50 s | `Sam опубликует build WP-129 в 15:30 с бюджетом 42 USD.` | Go; opt-in Quality |

Additional cache test results were Fast 2.72 s cold / 0.82 s warm, Qwen3.8
4.05 s / 1.68 s, and Gemma 4.77 s / 3.67 s for the compact MFU case. Direct
llama.cpp generation measured approximately 57.6, 36.4, and 35.5 generated
tokens/s respectively. These are guidance for this hardware, not cross-device
guarantees.

The sustained-concurrency gate also ran Fast and the production Whisper
large-v3-turbo Q8 Metal regression simultaneously. Whisper completed its real
decode in 10.37 s with a non-empty transcript and Metal initialization; Fast
completed the full multilingual corpus in 12.01 s with all protected tokens
and language gates intact. Both processes exited successfully under shared
unified-memory and GPU load.

Qwen3.5 and Qwen3.8 passed only after the pinned ChatML formatter retained an
explicit completed empty-think prefill. Gemma's full embedded Jinja is not
supported by the compact template API; its canonical no-thinking `<|turn>`
protocol was extracted from the pinned GGUF metadata and implemented directly.
Early MFU termination uses the first balanced JSON object. A defense-in-depth
filter removes think, channel, fence, and end-of-turn syntax before any result
can be displayed or persisted.

Peak-memory figures from llama.cpp's process report were approximately 8.9 GB
for both Qwen files and 5.0 GB for Gemma on this machine. The UI guidance stays
more conservative (12 GB Fast, 24 GB Quality) because Whisper and application
memory may coexist and macOS unified-memory pressure varies by device.

## Recommendation

Promote all three independently. Qwen3.5 is the default Fast profile;
Qwen3.8 and Gemma are explicit Quality choices. Keep Qwen3 legacy
selections addressable, never auto-download a replacement, and reject a model
job without altering its transcript when loading, validation, cancellation, or
resource checks fail.
