# Phase 4 Qwen3-ASR qualification

> **Current runtime note (2026-09-12):** the pure-Rust Qwen3-ASR 0.6B/1.7B
> experiment documented below is retired and its harness is no longer shipped.
> ADR-019 adopts only the official Qwen3-ASR 1.7B Q8_0 GGUF text/projector pair
> through `llama.cpp` MTMD, with one selection for all three modes. Its
> real-Metal integration smoke gate passes; a full comparative WER, latency,
> and sustained-session benchmark remains outstanding.

Date: 2026-09-12. Hardware: Apple M5 Pro, 48 GiB unified memory. OS: macOS
26.6.2. Rust: 1.97.1 aarch64-apple-darwin. All audio remained local.

## Recorded inputs and runtimes

Generate the corpus with:

```bash
scripts/generate-asr-benchmark-corpus.sh /tmp/whisper-pilot-asr-corpus
```

The retired experiment wrapped each decoder with `/usr/bin/time -l` for
single-process peak RSS and repeated samples for the stability gate (191
Russian iterations equal 30.04 input minutes). Those decoder harnesses were
removed with the pure-Rust runtime; the recorded results below are retained as
historical evidence rather than a currently runnable product gate.

The generator writes `manifest.tsv` with exact references, durations, and
hashes for every run. For the recorded run the mono 16 kHz WAV manifest was:

| file | duration | SHA-256 |
| --- | ---: | --- |
| `ru.wav` | 9.435375s | `22f4ace18ef5bc63609a04c08d7ddf4f473d83cfbb370f9fb390cb8b0a4d57e3` |
| `en.wav` | 8.115313s | `8cb18bdebd3bfa155270e2654015bf3b1c95f5c58d77c2cf15ba0fd40237e361` |
| `mixed.wav` | 6.612250s | `282e4e4f204197b4abd553de55ab889429c92cd33269c1fc03f59f06c11f2269` |

Normalized references are the literal Russian and English sentences in
`generate-asr-benchmark-corpus.sh`; mixed concatenates its two literal clauses.
Scoring lowercased and removed punctuation. Raw WER intentionally did not
normalize spoken numbers to digits, so Whisper's semantically correct number
formatting appears as substitutions. The corpus script remains checked in, but
decoder commands, hypotheses, and the ad-hoc WER scorer were not retained. The WER,
three-session RSS, native-streaming, and 60-second rows below are therefore
recorded qualification evidence, not a claim that one checked-in command
recreates every headline number. The hashes are specific to the recorded macOS
voices; a future OS voice update requires a new manifest.

- Whisper: `whisper-rs` 0.16.0, large-v3-turbo Q8, SHA-256
  `317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1`.
- Qwen runtime: pure-Rust `qwen-asr` 0.11.0, Accelerate + vDSP.
- Qwen 0.6B: official revision
  `5eb144179a02acc5e5ba31e748d22b0cf3e303b0`, 1,880,540,390-byte bundle.
- Qwen 1.7B: official revision
  `7278e1e70fe206f11671096ffdd38061171dd6e5`; exact bundle size was not retained
  as promotion evidence because this variant was rejected.

## Results

Times are warm decoder compute on the 8–10 second files. The 0.5-second probe is
only the compute cost of calling Qwen's streaming API after 8,000 samples; its
returned text was not retained and may have been empty. It is not a measured
first non-empty partial latency. Peak RSS for the multi-session Qwen row came
from a separate ad-hoc three-shared-session run; the checked-in harness measures
one context (or repeated sequential decodes). Whisper uses one context.

| Engine | RU WER | EN WER | mixed WER | RU/EN final | 0.5s probe compute | RTF RU/EN | peak RSS | timestamps |
| --- | ---: | ---: | ---: | --- | --- | --- | ---: | --- |
| Whisper turbo Q8 | 25.0% raw, 0% number-normalized | 29.2% raw, 0% number-normalized | 72.2% | 1.08s / 1.03s | not measured | 0.115 / 0.127 | 1.16 GiB | yes |
| Qwen3-ASR 0.6B | 0% | 0% | 66.7% | 0.85s / 0.60s | 0.22–0.25s | 0.090 / 0.073 | 4.23 GiB; 3.50 GiB single-session | no |
| Qwen3-ASR 1.7B | 0% | 0% | 44.4% | 2.20s / 1.35s | 0.55–0.72s | 0.234 / 0.167 | 8.29 GiB | no |

Mixed-language WER understates the failure: 0.6B translated the Russian clause
to English, 1.7B translated the English clause to Russian, and Whisper omitted
the English clause. None preserves the frozen code-switch input, so none is
accepted as a mixed-language guarantee. Whisper remains the product default
because it retains timestamps, auto detection, established Metal behavior, and
all modes; mixed speech remains explicitly best-effort.

The 0.6B bounded-window path completed 191 consecutive Russian decodes —
30.04 minutes of equivalent input — in 162.82 seconds with no empty result or
crash and 3.50 GiB peak RSS. A direct 60-second segmented decode, made by
looping/trimming `ru.wav` with ffmpeg and run through the same Qwen harness,
crashed inside
`qwen-asr` 0.11.0 (`range start index 30 out of range for slice of length 25`);
the 1.7B long-input attempt failed the same gate. This failure is retained as a
hard boundary for that rejected pure-Rust runtime. The production GGUF/MTMD
adapter remains bounded: natural live utterances have a 20-second hard cap and
file-quality decode uses 30-second windows, never an unbounded input.

Qwen native streaming returned the full English sample but truncated the
Russian tail in both sizes under the tested parameters. Production therefore
uses qualified fast offline decode on Recorder's stable bounded windows, not
the candidate runtime's provisional streaming API. The standalone ASR models
return no timestamps. The separate ForcedAligner was not promoted or measured:
it would add another unqualified bundle and latency to a mode whose capture
window already supplies the required stable span.

## Qwen3-ASR 1.7B GGUF integration evidence

The production `llama-cpp-2` MTMD decoder was run on Apple M5 Pro with the
pinned `Qwen3-ASR-1.7B-Q8_0.gguf` backbone and
`mmproj-Qwen3-ASR-1.7B-Q8_0.gguf` projector after both SHA-256 checks matched
the catalog. Metal initialized, the audio projector encoded the input, and a
single RU→EN code-switch fixture completed successfully in 4.80 seconds after
model load. The 8.67-second synthesized input produced both Cyrillic Russian
and English text in the same result. Two proper nouns were imperfect
(`Whisper` → `Vesper`, `Qwen` → `Quan`), so this is integration evidence, not
a quality-equivalence claim.

The reusable opt-in gate is:

```bash
QWEN3_ASR_GGUF_MODEL=/path/to/Qwen3-ASR-1.7B-Q8_0.gguf \
QWEN3_ASR_GGUF_MMPROJ=/path/to/mmproj-Qwen3-ASR-1.7B-Q8_0.gguf \
QWEN3_ASR_GGUF_TEST_AUDIO=/path/to/mixed.wav \
  cargo test --manifest-path src-tauri/Cargo.toml \
  qwen_gguf_asr::tests::real_qwen_gguf_audio_smoke_when_fixture_paths_are_available \
  -- --nocapture
```

## Acceptance and recommendation

Numerical gates for the original optional Recorder engine were: monolingual RU
and EN WER at most 10% on the recorded corpus, RTF below 0.5, bounded-window
final decode below the then-seven-second capture cadence, no empty/crashed iteration in a
30-minute equivalent stability run, complete
verified bundle, and peak RSS compatible with the 48 GiB baseline. Mixed mode
requires faithful preservation of both languages; timestamps and automatic
language detection are hard requirements only for modes that advertise them.
Real non-empty first-partial latency remains unverified and is not a satisfied
acceptance gate; production presents bounded-window results rather than Qwen's
unqualified native streaming output.

The later natural-utterance live policy (two-second partial cadence, 600 ms
pause commit, 20-second hard cap) and Recorder's 30-second GGUF quality pass
still require a repeated real-Metal latency/stability run; unit tests cover the
window boundaries and fallback behavior but do not replace that hardware gate.

- Retire Qwen3-ASR 0.6B because it does not satisfy the shared all-mode and
  mixed-language product boundary.
- The original pure-Rust Qwen3-ASR 1.7B candidate was rejected; the later
  GGUF/MTMD implementation is the sole selectable Qwen engine for all three
  modes under ADR-019.
- Retain Whisper as installed default and explicit fallback; never substitute
  it silently after a Qwen session starts.
