# ADR-004: ffmpeg as the single audio/video ingestion path

- **Status:** accepted
- **Date:** 2026-07-21
- **Deciders:** Alexey Platkovsky

## Context

Input files can be audio (mp3, wav, m4a, …) or video (mp4, mov, …), in many
codecs and sample rates. Whisper requires 16 kHz mono PCM. The original framing
was "if video, extract audio; if audio, use it directly."

## Decision

Normalize **every** input through one ffmpeg invocation
(`-vn -ac 1 -ar 16000 -f wav`) to a temporary 16 kHz mono WAV, then decode it
with `hound`. ffmpeg extracts audio from video and resamples audio identically,
so no branch on file type is needed. The temporary WAV is deleted after decoding.

## Consequences

- One code path for all inputs; broad format support for free.
- ffmpeg becomes a required dependency. For M1 it is a system binary on PATH
  (`brew install ffmpeg`); bundling a sidecar is a later hardening step.
- A clear, actionable error is surfaced when ffmpeg is missing.

## Alternatives Considered

- **Branch on file type; decode audio in-process (symphonia)** — more code, more
  codecs to support ourselves, and still needs a video path. ffmpeg subsumes it.
- **Bundle ffmpeg from day one** — deferred; acceptable to require the system
  binary during early milestones.
