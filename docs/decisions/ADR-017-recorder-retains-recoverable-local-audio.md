# ADR-017: Recorder retains recoverable local audio

- **Status:** accepted
- **Date:** 2026-09-12
- **Deciders:** Alexey Platkovsky
- **Relates to:** [ADR-008](ADR-008-persisted-library.md) for Transcription's
  reference-only source-file policy. Recorder owns the audio it captures and
  therefore uses a different lifecycle.

## Context

Recorder is a third live mode for dictating Russian, English, or mixed speech.
Unlike Meeting, its purpose includes reopening and listening to a voice note,
not only reading the transcript. A live microphone stream can also end because
of a crash, device loss, full disk, or failed final transcription. Treating the
transcript as the only durable artifact would make those failures destructive
and would make the feature a dictation surface rather than a recorder.

Recorder therefore needs explicit ownership, format, commit, recovery, export,
and deletion rules. These rules must not change Transcription's reference-only
source files or Meeting's no-retained-audio contract.

## Decision

**Recorder retains both microphone audio and the transcript locally until the
user deletes the session.** WhisperPilot owns the captured audio artifact.
Transcript edits, copy, text export, and future local-LLM polishing never rewrite
that artifact.

The capture artifact is mono signed-16 PCM in a Core Audio Format file at the
input device's actual capture rate. WhisperPilot does not upsample a lower-rate
Bluetooth or USB source merely to label it 48 kHz; a 44.1 or 48 kHz source keeps
its available bandwidth, while a 16 or 24 kHz source remains at that rate.
Capture writes `<session>.caf.partial`; the writer checkpoints often enough to
bound unflushed audio to one second. A successful Stop enters an authoritative
Rust `Finalizing` phase which retains the shared live-source lock while it
performs, in order:

1. transcribe and persist meaningful trailing speech;
2. flush, `fsync`, and close the audio file;
3. atomically rename `.caf.partial` to `.caf`;
4. commit the completed Recorder row in the local database.

Recovery reconciles filesystem and database state on launch. A `finalizing` row
with a final `.caf`, or a completed row with only `.caf.partial`, is preserved and
offered as a recoverable session; neither mismatch is silently deleted. An
interrupted `.partial` remains available for **Recover** or **Delete**. Failed
cleanup leaves a visible **Delete failed / Retry** state. Files explicitly
exported by the user are outside application ownership and are never removed by
session deletion.

Playback uses the retained CAF. **Export WAV** creates a separate PCM WAV copy at
the user-selected destination and preserves the stored sample rate. Automatic
expiry, background cloud upload, audio editing, and live Recorder diarization
are outside this decision.

The native-rate mono stream is the Recorder master. Each speech engine receives
a separate band-limited derivative at the rate required by its transport or
model: local Whisper receives 16 kHz, OpenAI Realtime receives 24 kHz, and a
provider that accepts the actual source rate may receive it without a redundant
conversion. Accepting 48 kHz is not treated as proof of better recognition;
provider-specific RU/EN evaluation decides whether sending wideband audio is
worth its additional bandwidth. MFU, translation, and text polishing consume
transcript text and therefore do not receive an audio derivative.

Start uses a preflight transaction boundary. Permission, selected Whisper model,
the system-default input device, and any required shortcut caption surface are
validated before a durable session or `.partial` is created. Failed preflight is
shown as an actionable error and is not added to history. Recorder v1 binds the
device selected as system default at Start; a later default-device change does
not migrate an active stream, while loss of the active device stops capture into
a recoverable error.

## Consequences

- Recorder can satisfy playback and crash-recovery expectations without changing
  Transcription or Meeting audio ownership.
- A session comprises coordinated database and filesystem resources, so startup
  reconciliation and failure-injection tests are required.
- CAF is the durable macOS capture format; WAV is an interoperability export,
  avoiding repeated WAV-header rewrites during capture. Sessions may have
  different declared sample rates, so playback/export must read container
  metadata rather than assume 16 kHz.
- Preserving the native-rate master avoids irreversible quality loss in saved
  voice notes. Local Whisper and fixed-rate cloud transports still require a
  tested band-limited streaming resampler.
- The `Finalizing` phase is observable and mutually exclusive with another live
  capture start. The app must not show an idle or completed state before both
  trailing text and audio reach their durable boundary.
- Local storage can grow without automatic retention. The UI must show ownership
  clearly and make deletion explicit.

## Alternatives Considered

- **Persist transcript only** — rejected because it does not meet the confirmed
  recorder requirement and cannot support playback or audio recovery.
- **Normalize and retain every recording at 16 kHz** — rejected because it
  irreversibly removes bandwidth from common 44.1/48 kHz microphones and makes
  the saved voice note lower fidelity than the captured source.
- **Upsample every recording to a fixed 48 kHz master** — rejected because it
  adds storage and conversion without adding information to lower-rate devices.
- **Write WAV directly while capturing** — rejected because CAF provides a more
  natural crash-tolerant container for the macOS-first runtime. WAV remains an
  explicit export format.
- **Keep audio temporarily and delete after transcription** — rejected because a
  reopened voice note would lose its source and playback would be impossible.
- **Store audio outside the session lifecycle** — rejected because orphan cleanup
  and user-visible ownership would become ambiguous.
