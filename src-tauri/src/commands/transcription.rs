//! Transcription IPC commands: transcribe a meeting's source file and re-run
//! speaker diarization alone.

use crate::audio;
use crate::diarize;
use crate::diarize_process;
use crate::error::{AppError, Result};
use crate::events::{TranscriptionPhaseEvent, TranscriptionProgressEvent};
use crate::meetings::MeetingDto;
use crate::settings;
use crate::state::{app_data_dir, AppState};
use crate::streaming_session;
use crate::transcribe;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, State};
use whisper_rs::WhisperContext;

enum MeetingDecoderModel {
    Whisper(Arc<WhisperContext>),
    #[cfg(target_os = "macos")]
    QwenGguf(Arc<crate::qwen_gguf_asr::QwenGgufAsrModel>),
}

/// `transcribe_meeting`'s result: the persisted meeting plus a non-fatal
/// warning when diarization was requested but degraded (its active model's
/// file was missing or corrupt) — the transcription itself always succeeds
/// with plain, speaker-less segments in that case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscribeMeetingResult {
    pub meeting: MeetingDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diarization_warning: Option<String>,
}

/// The persisted `active_model.diarization` setting names the embedding
/// variant to run, or the literal `"none"` to skip diarization entirely.
fn diarization_variant_to_run(setting: &str) -> Option<&str> {
    if setting == "none" {
        None
    } else {
        Some(setting)
    }
}

/// What a spawned diarization task hands back: the engine's own result and an
/// optional fallback warning (set when the other embedding model was retried
/// after a crash on this recording), or the `JoinError` from the blocking task
/// panicking or being cancelled.
type DiarizationOutcome = std::result::Result<
    (Result<Vec<diarize::SpeakerTurn>>, Option<String>),
    tokio::task::JoinError,
>;

/// A not-yet-started diarization pass. Boxed rather than generic so the
/// "no active model" case is a plain `None` at every call site, and deferred so
/// that nothing in it — not even the phase event — can run before the
/// transcript is persisted.
type PendingDiarization =
    std::pin::Pin<Box<dyn std::future::Future<Output = DiarizationOutcome> + Send>>;

/// Move one decoded allocation into blocking work and return that same
/// allocation for the next pipeline stage. This avoids cloning an entire
/// recording merely to satisfy the blocking task's `'static` ownership.
async fn transcribe_owned_samples<T, F>(samples: Vec<f32>, transcribe: F) -> Result<(T, Vec<f32>)>
where
    T: Send + 'static,
    F: FnOnce(&[f32]) -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let transcription = transcribe(&samples)?;
        Ok((transcription, samples))
    })
    .await
    .map_err(|error| AppError::Transcribe(error.to_string()))?
}

/// Decode and transcribe the file at `path`, returning the transcription and
/// the samples it was decoded from so diarization can reuse them.
///
async fn decode_and_transcribe(
    app: tauri::AppHandle,
    id: i64,
    model: MeetingDecoderModel,
    path: String,
) -> Result<(transcribe::Transcription, Vec<f32>)> {
    let input = PathBuf::from(&path);

    // Decode once (off the reactor); both transcription and diarization run
    // over the same samples.
    let samples = tokio::task::spawn_blocking(move || audio::load_samples(&input))
        .await
        .map_err(|e| AppError::Transcribe(e.to_string()))??;
    let (transcription, samples) = transcribe_owned_samples(samples, move |samples| {
        let emit_progress = |percent| {
            let _ = app.emit(
                "transcription_progress",
                TranscriptionProgressEvent { id, percent },
            );
        };
        match model {
            MeetingDecoderModel::Whisper(ctx) => {
                transcribe::transcribe_with_progress(&ctx, samples, emit_progress)
            }
            #[cfg(target_os = "macos")]
            MeetingDecoderModel::QwenGguf(model) => {
                transcribe_qwen_recording(&model, samples, emit_progress)
            }
        }
    })
    .await?;
    ensure_non_empty_transcript(&transcription)?;

    Ok((transcription, samples))
}

#[cfg(target_os = "macos")]
pub(crate) fn transcribe_qwen_recording(
    model: &crate::qwen_gguf_asr::QwenGgufAsrModel,
    samples: &[f32],
    mut on_progress: impl FnMut(i32),
) -> Result<transcribe::Transcription> {
    let windows = qwen_window_plan(samples.len());
    let total_windows = windows.len().max(1);
    let mut segments = Vec::new();
    let mut language = "auto".to_string();
    let mut confirmed_context = String::new();
    for (index, &(decode_start, decode_end, logical_start, logical_end)) in
        windows.iter().enumerate()
    {
        let decoded = model.transcribe_window_with_context(
            &samples[decode_start..decode_end],
            (!confirmed_context.is_empty()).then_some(confirmed_context.as_str()),
        )?;
        if language == "auto" && decoded.language != "auto" {
            language = decoded.language;
        }
        let decoded_text = decoded
            .segments
            .iter()
            .map(|segment| segment.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        place_qwen_window_segments(
            &mut segments,
            decoded.segments,
            decode_start,
            logical_start,
            logical_end,
        );
        append_confirmed_context(&mut confirmed_context, &decoded_text);
        on_progress((((index + 1) * 100 / total_windows) as i32).min(100));
    }
    Ok(transcribe::Transcription { segments, language })
}

/// Place Qwen's window-relative timestamps back onto the source timeline.
///
/// Decode windows include one second of prior audio for ASR context, but only
/// their logical range contributes new transcript. We therefore offset by the
/// *decode* start (not the logical start), discard segments wholly in the
/// prepended overlap, and clip boundary-crossing ranges without trying to
/// split their text. Exact repeated overlap output is suppressed against the
/// preceding committed segment.
#[cfg(target_os = "macos")]
fn place_qwen_window_segments(
    committed: &mut Vec<transcribe::Segment>,
    decoded: Vec<transcribe::Segment>,
    decode_start: usize,
    logical_start: usize,
    logical_end: usize,
) {
    let decode_start_ms = samples_to_ms(decode_start);
    let logical_start_ms = samples_to_ms(logical_start);
    let logical_end_ms = samples_to_ms(logical_end);

    for mut segment in decoded {
        let original_start_ms = decode_start_ms.saturating_add(segment.start_ms);
        let original_end_ms = decode_start_ms.saturating_add(segment.end_ms);
        if original_end_ms <= logical_start_ms || original_start_ms >= logical_end_ms {
            continue;
        }

        let start_ms = original_start_ms.max(logical_start_ms);
        let end_ms = original_end_ms.min(logical_end_ms).max(start_ms);
        if is_repeated_qwen_overlap(committed, &segment.text, original_start_ms, original_end_ms) {
            continue;
        }

        segment.start_ms = start_ms;
        segment.end_ms = end_ms;
        committed.push(segment);
    }
}

#[cfg(target_os = "macos")]
fn samples_to_ms(samples: usize) -> u64 {
    samples as u64 * 1_000 / crate::audio::SAMPLE_RATE as u64
}

#[cfg(target_os = "macos")]
fn is_repeated_qwen_overlap(
    committed: &[transcribe::Segment],
    candidate_text: &str,
    candidate_start_ms: u64,
    candidate_end_ms: u64,
) -> bool {
    let Some(previous) = committed.last() else {
        return false;
    };
    normalize_qwen_segment_text(&previous.text) == normalize_qwen_segment_text(candidate_text)
        && candidate_start_ms <= previous.end_ms
        && previous.start_ms <= candidate_end_ms
}

#[cfg(target_os = "macos")]
fn normalize_qwen_segment_text(text: &str) -> String {
    text.split_whitespace()
        .flat_map(str::chars)
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(target_os = "macos")]
fn qwen_window_plan(sample_count: usize) -> Vec<(usize, usize, usize, usize)> {
    const WINDOW_SAMPLES: usize = crate::audio::SAMPLE_RATE as usize * 30;
    const OVERLAP_SAMPLES: usize = crate::audio::SAMPLE_RATE as usize;
    let mut windows = Vec::new();
    let mut logical_start = 0;
    while logical_start < sample_count {
        let decode_start = logical_start.saturating_sub(OVERLAP_SAMPLES);
        let new_sample_capacity = WINDOW_SAMPLES - logical_start.saturating_sub(decode_start);
        let logical_end = logical_start
            .saturating_add(new_sample_capacity)
            .min(sample_count);
        windows.push((decode_start, logical_end, logical_start, logical_end));
        logical_start = logical_end;
    }
    windows
}

#[cfg(target_os = "macos")]
fn append_confirmed_context(context: &mut String, decoded: &str) {
    const MAX_CONTEXT_CHARS: usize = 240;
    if decoded.is_empty() {
        return;
    }
    if !context.is_empty() {
        context.push(' ');
    }
    context.push_str(decoded);
    let count = context.chars().count();
    if count > MAX_CONTEXT_CHARS {
        *context = context
            .chars()
            .skip(count - MAX_CONTEXT_CHARS)
            .collect::<String>()
            .trim_start()
            .to_string();
    }
}

/// Reject empty Meeting decodes before persistence or diarization. This
/// protects against the Metal encoder's empty-output failure while allowing
/// Streaming windows to remain tolerant of silence.
fn ensure_non_empty_transcript(transcription: &transcribe::Transcription) -> Result<()> {
    if transcription.segments.is_empty() {
        return Err(AppError::Transcribe(
            "The selected transcription model decoded no speech from this file. Check that the source contains \
             audible speech and try another recording if needed."
                .to_string(),
        ));
    }
    Ok(())
}

/// Persist `transcription` immediately, then run `diarization` (when a
/// model is active) and write the speaker ids as a second, separate save —
/// load-bearing, since diarization's native code can abort the process past
/// any Rust error path (see `docs/architecture.md`'s Speaker Diarization
/// section). A diarization failure degrades to the already-persisted
/// speaker-less segments as a warning, never failing the transcription.
pub(crate) async fn persist_transcript_then_diarize(
    app_support_dir: PathBuf,
    meeting_id: i64,
    mut transcription: transcribe::Transcription,
    diarization: Option<PendingDiarization>,
) -> Result<(MeetingDto, Option<String>)> {
    let duration_ms = transcription
        .segments
        .last()
        .map(|segment| segment.end_ms as i64);
    let language = transcription.language.clone();

    let meeting = crate::meetings::save_transcript(
        &app_support_dir,
        meeting_id,
        segment_dtos(&transcription.segments),
        duration_ms,
        language.clone(),
    )?;

    let Some(diarization) = diarization else {
        return Ok((meeting, None));
    };

    let outcome = diarization.await;
    let speakers_assigned = matches!(&outcome, Ok((Ok(_), _)));
    let warning = diarize::apply_diarization_outcome(&mut transcription.segments, outcome);
    if !speakers_assigned {
        // Nothing was assigned, so the first save already holds the final state.
        return Ok((meeting, warning));
    }

    // The transcript is already safe, so a failed speaker-id write degrades to
    // the same warning as any other diarization failure instead of reporting a
    // failed transcription.
    match crate::meetings::save_transcript(
        &app_support_dir,
        meeting_id,
        segment_dtos(&transcription.segments),
        duration_ms,
        language,
    ) {
        Ok(meeting) => Ok((meeting, warning)),
        Err(e) => {
            log::warn!("could not persist speaker ids, transcript kept as-is: {e}");
            Ok((
                meeting,
                Some(format!("Speaker identification could not be saved: {e}")),
            ))
        }
    }
}

fn segment_dtos(segments: &[transcribe::Segment]) -> Vec<crate::meetings::SegmentDto> {
    segments
        .iter()
        .map(|segment| crate::meetings::SegmentDto {
            start_ms: segment.start_ms as i64,
            end_ms: segment.end_ms as i64,
            text: segment.text.clone(),
            speaker_id: segment.speaker_id.map(i64::from),
        })
        .collect()
}

/// Resolve the selected file-transcription ASR while preventing a concurrent
/// Settings/Delete command from changing that selection or deleting its
/// assets before this invocation records an active decoder owner.
async fn resolve_meeting_asr_and_claim<'a>(
    state: &'a AppState,
    app_support_dir: &std::path::Path,
) -> Result<(
    settings::Settings,
    &'static crate::asr::AsrModelSpec,
    streaming_session::WhisperUsageGuard<'a>,
)> {
    let mutation = state.recorder_asr_mutation.lock().await;
    let app_settings = settings::get_settings(app_support_dir);
    let model_id = app_settings
        .active_model_transcription
        .as_deref()
        .unwrap_or(crate::asr::DEFAULT_ASR_MODEL_ID);
    let asr_spec = crate::asr::resolve_selection(
        model_id,
        crate::asr::AsrMode::Meeting,
        crate::asr::AsrLanguage::Auto,
    )?;
    let usage = streaming_session::WhisperUsageGuard::acquire(
        &state.whisper_busy,
        streaming_session::WhisperUser::Meeting,
    )
    .map_err(|holder| match holder {
        streaming_session::WhisperUser::Streaming => AppError::Transcribe(
            "a Meeting is active; stop it before starting a file transcription".into(),
        ),
        streaming_session::WhisperUser::Recorder => AppError::Transcribe(
            "Recorder is active; stop it before starting a file transcription".into(),
        ),
        streaming_session::WhisperUser::Meeting => {
            AppError::Transcribe("another file transcription is already running".into())
        }
    })?;
    // A successful claim makes the exclusive owner visible to mutation
    // commands, so no lock has to remain held during slow model work.
    drop(mutation);
    Ok((app_settings, asr_spec, usage))
}

/// Attach (or clear, when `path` is `None`) the source file of a meeting.
/// Selecting the file is separate from running the transcription.
#[tauri::command]
pub(crate) fn set_meeting_source(
    app: tauri::AppHandle,
    id: i64,
    path: Option<String>,
) -> Result<MeetingDto> {
    crate::meetings::set_meeting_source(&app_data_dir(&app)?, id, path)
}

/// Transcribe the meeting's attached source file into timestamped segments and
/// persist the result against the meeting. Whisper detects the language itself;
/// the meeting's stored `language` is an output of that decode, never an input
/// to it, so a value left by an earlier run does not influence this one.
#[tauri::command]
pub(crate) async fn transcribe_meeting(
    app: tauri::AppHandle,
    id: i64,
    state: State<'_, AppState>,
) -> Result<TranscribeMeetingResult> {
    let app_support_dir = app_data_dir(&app)?;
    // WP-71: the returned usage guard serializes this work with Meeting and
    // Recorder capture until the command returns.
    let (app_settings, asr_spec, _whisper_guard) =
        resolve_meeting_asr_and_claim(&state, &app_support_dir).await?;

    let meeting = crate::meetings::open_meeting(&app_support_dir, id)?;
    let path = meeting.source_path.ok_or_else(|| {
        AppError::Transcribe("transcription has no source file to transcribe".to_string())
    })?;

    let active_diarization_variant =
        diarization_variant_to_run(&app_settings.active_model_diarization).map(str::to_string);

    let decoder_model = match asr_spec.runtime {
        crate::asr::AsrRuntime::WhisperCpp => state
            .model(app_support_dir.clone())
            .await
            .map(MeetingDecoderModel::Whisper),
        #[cfg(target_os = "macos")]
        crate::asr::AsrRuntime::LlamaCppMtmd => state
            .qwen_gguf_asr_model(app_support_dir.clone(), asr_spec)
            .await
            .map(MeetingDecoderModel::QwenGguf),
        #[cfg(not(target_os = "macos"))]
        crate::asr::AsrRuntime::LlamaCppMtmd => Err(AppError::InvalidSetting(
            "Qwen3-ASR GGUF is currently available on macOS".into(),
        )),
    }?;
    let (transcription, samples) =
        decode_and_transcribe(app.clone(), id, decoder_model, path).await?;

    let diarization: Option<PendingDiarization> = active_diarization_variant.map(|variant| {
        let app = app.clone();
        let app_support_dir = app_support_dir.clone();
        Box::pin(async move {
            // Lets the UI switch its status from "Transcribing" to "Diarizing"
            // instead of showing one label across two distinct,
            // separately-timed passes. Emitted here, inside the deferred pass,
            // so it cannot announce diarization before the transcript is safe.
            let _ = app.emit(
                "transcription_phase",
                TranscriptionPhaseEvent {
                    id,
                    phase: "diarizing",
                },
            );

            // Runs in a child process: the native engine can abort outright,
            // and a fatal signal is not something `spawn_blocking` or any Rust
            // error path can catch. Isolating it turns that abort into an
            // ordinary error. When the crash is a known native fault of one
            // embedding model, the call retries once with the other model
            // before failing open — see `diarize_process::diarize_with_fallback`
            // and docs/architecture.md's Speaker Diarization section.
            tokio::task::spawn_blocking(move || {
                diarize_process::diarize_with_fallback(&app_support_dir, samples, None, &variant)
            })
            .await
        }) as PendingDiarization
    });

    let (meeting, diarization_warning) =
        persist_transcript_then_diarize(app_support_dir, id, transcription, diarization).await?;

    Ok(TranscribeMeetingResult {
        meeting,
        diarization_warning,
    })
}

/// Re-run speaker identification alone on an already-transcribed meeting,
/// leaving the transcript text untouched — the "Diarize" header action.
/// Unlike the diarization pass folded into `transcribe_meeting`, a failure
/// here is a real error (the user asked for diarization specifically, so
/// there is no already-safe transcript to fail open onto); a fallback
/// warning (the other embedding model was retried after a crash) still
/// succeeds and is surfaced the same way `transcribe_meeting` does.
#[tauri::command]
pub(crate) async fn diarize_meeting(
    app: tauri::AppHandle,
    id: i64,
) -> Result<TranscribeMeetingResult> {
    let app_support_dir = app_data_dir(&app)?;
    let meeting = crate::meetings::open_meeting(&app_support_dir, id)?;
    if meeting.segments.is_empty() {
        return Err(AppError::Diarization(
            "transcription has no transcript to diarize yet".into(),
        ));
    }
    if meeting.source_missing {
        return Err(AppError::Diarization(
            "transcription's source file is missing".into(),
        ));
    }
    let path = meeting.source_path.ok_or_else(|| {
        AppError::Diarization("transcription has no source file to diarize".to_string())
    })?;
    let variant = diarization_variant_to_run(
        &settings::get_settings(&app_support_dir).active_model_diarization,
    )
    .map(str::to_string)
    .ok_or_else(|| AppError::Diarization("no diarization model is active".into()))?;

    let input = PathBuf::from(&path);
    let samples = tokio::task::spawn_blocking(move || audio::load_samples(&input))
        .await
        .map_err(|e| AppError::Diarization(e.to_string()))??;

    let diarize_dir = app_support_dir.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        diarize_process::diarize_with_fallback(&diarize_dir, samples, None, &variant)
    })
    .await;

    match outcome {
        Ok((Ok(turns), fallback_warning)) => {
            let meeting = crate::meetings::diarize_meeting_segments(&app_support_dir, id, &turns)?;
            Ok(TranscribeMeetingResult {
                meeting,
                diarization_warning: fallback_warning,
            })
        }
        Ok((Err(e), _fallback_warning)) => Err(AppError::Diarization(format!(
            "speaker identification is unavailable: {e}"
        ))),
        Err(e) => Err(AppError::Diarization(format!(
            "speaker identification failed: {e}"
        ))),
    }
}

#[cfg(test)]
#[path = "transcription_tests.rs"]
mod tests;
