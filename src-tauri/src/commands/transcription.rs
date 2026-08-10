//! Transcription IPC commands: transcribe a meeting's source file, cancel an
//! in-flight run (WP-19), and re-run speaker diarization alone.

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

/// Decode and transcribe the file at `path`, returning the transcription and
/// the samples it was decoded from so diarization can reuse them.
///
/// Checked against `cancel` both before spawning the (potentially long) whisper
/// decode and inside it (WP-19), so a Stop clicked while the file is still
/// being decoded to samples is not lost waiting for whisper to start.
async fn decode_and_transcribe(
    ctx: Arc<WhisperContext>,
    path: String,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    on_progress: impl FnMut(i32) + Send + 'static,
) -> Result<(transcribe::Transcription, Vec<f32>)> {
    let input = PathBuf::from(&path);

    // Decode once (off the reactor); both transcription and diarization run
    // over the same samples.
    let samples = tokio::task::spawn_blocking(move || audio::load_samples(&input))
        .await
        .map_err(|e| AppError::Transcribe(e.to_string()))??;
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }
    let transcription = {
        let samples = samples.clone();
        tokio::task::spawn_blocking(move || {
            transcribe::transcribe(&ctx, &samples, &cancel, on_progress)
        })
        .await
        .map_err(|e| AppError::Transcribe(e.to_string()))??
    };

    Ok((transcription, samples))
}

/// Registers `id`'s abort flag in `AppState.running_transcription` for this
/// run's lifetime and clears the slot on every exit path (success, error, or
/// unwind), so `cancel_transcription` can never target a run that has already
/// finished — and never clears a *different*, already-started run's slot.
pub(crate) struct TranscriptionCancelGuard<'a> {
    state: &'a AppState,
    id: i64,
}

impl<'a> TranscriptionCancelGuard<'a> {
    pub(crate) fn register(
        state: &'a AppState,
        id: i64,
    ) -> (Self, Arc<std::sync::atomic::AtomicBool>) {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        *state
            .running_transcription
            .lock()
            .expect("running_transcription lock poisoned") = Some((id, Arc::clone(&flag)));
        (Self { state, id }, flag)
    }
}

impl Drop for TranscriptionCancelGuard<'_> {
    fn drop(&mut self) {
        let mut guard = self
            .state
            .running_transcription
            .lock()
            .expect("running_transcription lock poisoned");
        if matches!(guard.as_ref(), Some((running_id, _)) if *running_id == self.id) {
            *guard = None;
        }
    }
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
    // WP-71: a Streaming session and a Meeting transcription share the one
    // cached Whisper context and cannot run concurrently. Held for this
    // whole command, released on return via Drop.
    let _whisper_guard = streaming_session::WhisperUsageGuard::acquire(
        &state.whisper_busy,
        streaming_session::WhisperUser::Meeting,
    )
    .map_err(|holder| match holder {
        streaming_session::WhisperUser::Streaming => AppError::Transcribe(
            "a Streaming session is active; stop it before transcribing a meeting".into(),
        ),
        streaming_session::WhisperUser::Meeting => {
            AppError::Transcribe("another meeting transcription is already running".into())
        }
    })?;

    let app_support_dir = app_data_dir(&app)?;
    let meeting = crate::meetings::open_meeting(&app_support_dir, id)?;
    let path = meeting.source_path.ok_or_else(|| {
        AppError::Transcribe("meeting has no source file to transcribe".to_string())
    })?;

    let active_diarization_variant = diarization_variant_to_run(
        &settings::get_settings(&app_support_dir).active_model_diarization,
    )
    .map(str::to_string);

    let (_cancel_guard, cancel) = TranscriptionCancelGuard::register(state.inner(), id);
    let ctx = state.model(app_support_dir.clone()).await?;
    let progress_app = app.clone();
    let on_progress = move |percent: i32| {
        let _ = progress_app.emit(
            "transcription_progress",
            TranscriptionProgressEvent { id, percent },
        );
    };
    let (transcription, samples) = decode_and_transcribe(ctx, path, cancel, on_progress).await?;

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
            "meeting has no transcript to diarize yet".into(),
        ));
    }
    if meeting.source_missing {
        return Err(AppError::Diarization(
            "meeting's source file is missing".into(),
        ));
    }
    let path = meeting.source_path.ok_or_else(|| {
        AppError::Diarization("meeting has no source file to diarize".to_string())
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

/// Stop the meeting's in-flight transcription (WP-19): flips its abort flag,
/// which whisper's abort callback (or the pre-decode check in
/// `decode_and_transcribe`) turns into `AppError::Cancelled` — the run then
/// returns before any transcript is persisted, so no document is created.
#[tauri::command]
pub(crate) fn cancel_transcription(id: i64, state: State<'_, AppState>) -> Result<()> {
    let guard = state
        .running_transcription
        .lock()
        .expect("running_transcription lock poisoned");
    match running_transcription_flag(&guard, id) {
        Some(flag) => {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }
        None => Err(AppError::Transcribe(format!(
            "no transcription is running for meeting {id}"
        ))),
    }
}

/// Pure decision behind `cancel_transcription`: `id`'s abort flag, only when
/// it is the currently running transcription.
fn running_transcription_flag(
    running: &Option<(i64, Arc<std::sync::atomic::AtomicBool>)>,
    id: i64,
) -> Option<Arc<std::sync::atomic::AtomicBool>> {
    match running {
        Some((running_id, flag)) if *running_id == id => Some(Arc::clone(flag)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // EP: "none" is the sole class that skips diarization; every other
    // string (however it got there) names a variant to run.
    #[test]
    fn diarization_variant_to_run_skips_when_setting_is_none() {
        assert_eq!(diarization_variant_to_run("none"), None);
    }

    #[test]
    fn diarization_variant_to_run_passes_through_a_real_variant() {
        assert_eq!(diarization_variant_to_run("campplus"), Some("campplus"));
        assert_eq!(
            diarization_variant_to_run("titanet-large"),
            Some("titanet-large")
        );
    }

    fn sample_meeting_dto() -> MeetingDto {
        MeetingDto {
            id: 1,
            title: "Test Meeting".to_string(),
            source_path: None,
            source_name: None,
            created_at_ms: 0,
            duration_ms: None,
            language: "en".to_string(),
            status: "transcribed".to_string(),
            segments: Vec::new(),
            notes: None,
            source_missing: false,
        }
    }

    #[test]
    fn transcribe_meeting_result_round_trips_with_a_diarization_warning() {
        let original = TranscribeMeetingResult {
            meeting: sample_meeting_dto(),
            diarization_warning: Some("active diarization model is missing".to_string()),
        };

        let json = serde_json::to_value(&original).unwrap();
        let round_tripped: TranscribeMeetingResult = serde_json::from_value(json).unwrap();

        assert_eq!(round_tripped, original);
    }

    #[test]
    fn transcribe_meeting_result_omits_diarization_warning_key_when_none() {
        let original = TranscribeMeetingResult {
            meeting: sample_meeting_dto(),
            diarization_warning: None,
        };

        let json = serde_json::to_value(&original).unwrap();

        assert!(json.get("diarization_warning").is_none());
    }

    fn segment(start_ms: u64, end_ms: u64, text: &str) -> transcribe::Segment {
        transcribe::Segment {
            start_ms,
            end_ms,
            text: text.to_string(),
            speaker_id: None,
        }
    }

    fn transcription(segments: Vec<transcribe::Segment>) -> transcribe::Transcription {
        transcribe::Transcription {
            segments,
            language: "en".to_string(),
        }
    }

    /// A meeting row to persist a transcript against, in a throwaway app-support
    /// directory — never the user's own.
    fn meeting_in(dir: &std::path::Path) -> i64 {
        crate::meetings::create_empty_meeting(dir, 0).unwrap().id
    }

    fn diarization(
        outcome: DiarizationOutcome,
    ) -> Option<std::pin::Pin<Box<dyn std::future::Future<Output = DiarizationOutcome> + Send>>>
    {
        Some(Box::pin(async move { outcome }))
    }

    // The whole point of WP-54: the transcript must already be readable from
    // the store at the moment diarization begins, so a diarization failure of
    // any kind — including a native crash that kills the process outright —
    // can only ever cost the speaker labels.
    #[tokio::test]
    async fn persist_transcript_then_diarize_persists_the_transcript_before_diarization_starts() {
        let dir = tempfile::tempdir().unwrap();
        let id = meeting_in(dir.path());
        let observed = Arc::new(std::sync::Mutex::new(None));

        let seen = Arc::clone(&observed);
        let path = dir.path().to_path_buf();
        let (_meeting, warning) = persist_transcript_then_diarize(
            dir.path().to_path_buf(),
            id,
            transcription(vec![
                segment(0, 1_000, "hello"),
                segment(2_000, 3_000, "world"),
            ]),
            Some(Box::pin(async move {
                // Reads the store from inside the diarization pass itself.
                *seen.lock().unwrap() = Some(crate::meetings::open_meeting(&path, id).unwrap());
                Ok((Ok(Vec::new()), None))
            })),
        )
        .await
        .unwrap();

        let at_diarization_time = observed.lock().unwrap().clone().expect("diarization ran");
        assert_eq!(at_diarization_time.segments.len(), 2);
        assert_eq!(at_diarization_time.segments[0].text, "hello");
        assert_eq!(at_diarization_time.language, "en");
        assert_eq!(at_diarization_time.duration_ms, Some(3_000));
        assert!(
            at_diarization_time
                .segments
                .iter()
                .all(|s| s.speaker_id.is_none()),
            "speaker ids are not known until diarization returns"
        );
        assert_eq!(warning, None);
    }

    #[tokio::test]
    async fn persist_transcript_then_diarize_keeps_the_persisted_transcript_when_diarization_fails()
    {
        let dir = tempfile::tempdir().unwrap();
        let id = meeting_in(dir.path());

        let (_meeting, warning) = persist_transcript_then_diarize(
            dir.path().to_path_buf(),
            id,
            transcription(vec![
                segment(0, 1_000, "hello"),
                segment(2_000, 3_000, "world"),
            ]),
            diarization(Ok((
                Err(AppError::Diarization("engine exploded".to_string())),
                None,
            ))),
        )
        .await
        .unwrap();

        assert!(warning.is_some(), "the failure is reported, not swallowed");
        let reopened = crate::meetings::open_meeting(dir.path(), id).unwrap();
        assert_eq!(reopened.segments.len(), 2);
        assert_eq!(reopened.segments[0].text, "hello");
        assert!(reopened.segments.iter().all(|s| s.speaker_id.is_none()));
    }

    #[tokio::test]
    async fn persist_transcript_then_diarize_writes_speaker_ids_onto_the_persisted_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let id = meeting_in(dir.path());
        // Distinct speakers, so the read-path coalescing in `to_dto` cannot
        // merge the two segments and hide a wrong assignment.
        let turns = vec![
            diarize::SpeakerTurn {
                start_ms: 0,
                end_ms: 1_000,
                speaker: 3,
            },
            diarize::SpeakerTurn {
                start_ms: 2_000,
                end_ms: 3_000,
                speaker: 4,
            },
        ];

        let (meeting, warning) = persist_transcript_then_diarize(
            dir.path().to_path_buf(),
            id,
            transcription(vec![
                segment(0, 1_000, "hello"),
                segment(2_000, 3_000, "world"),
            ]),
            diarization(Ok((Ok(turns), None))),
        )
        .await
        .unwrap();

        assert_eq!(warning, None);
        assert_eq!(meeting.segments[0].speaker_id, Some(3));
        assert_eq!(meeting.segments[1].speaker_id, Some(4));
        let reopened = crate::meetings::open_meeting(dir.path(), id).unwrap();
        assert_eq!(reopened.segments[0].speaker_id, Some(3));
        assert_eq!(reopened.segments[1].speaker_id, Some(4));
        assert_eq!(reopened.language, "en");
        assert_eq!(reopened.duration_ms, Some(3_000));
    }

    #[tokio::test]
    async fn persist_transcript_then_diarize_persists_the_transcript_when_no_model_is_active() {
        let dir = tempfile::tempdir().unwrap();
        let id = meeting_in(dir.path());

        let (meeting, warning) = persist_transcript_then_diarize(
            dir.path().to_path_buf(),
            id,
            transcription(vec![segment(0, 1_000, "hello")]),
            None,
        )
        .await
        .unwrap();

        assert_eq!(warning, None);
        assert_eq!(meeting.segments.len(), 1);
        let reopened = crate::meetings::open_meeting(dir.path(), id).unwrap();
        assert_eq!(reopened.segments.len(), 1);
        assert!(reopened.segments[0].speaker_id.is_none());
    }

    // Once the transcript is persisted, nothing downstream may turn into a
    // failed transcription — including the speaker-id write itself failing.
    #[tokio::test]
    async fn persist_transcript_then_diarize_warns_instead_of_failing_when_the_speaker_write_fails()
    {
        let dir = tempfile::tempdir().unwrap();
        let id = meeting_in(dir.path());
        let path = dir.path().to_path_buf();

        let (meeting, warning) = persist_transcript_then_diarize(
            dir.path().to_path_buf(),
            id,
            transcription(vec![segment(0, 1_000, "hello")]),
            Some(Box::pin(async move {
                // The meeting disappears after the transcript was persisted but
                // before the speaker ids can be written back to it.
                crate::meetings::delete_meeting(&path, id).unwrap();
                Ok((
                    Ok(vec![diarize::SpeakerTurn {
                        start_ms: 0,
                        end_ms: 1_000,
                        speaker: 1,
                    }]),
                    None,
                ))
            })),
        )
        .await
        .expect("a failed speaker-id write must not fail the transcription");

        assert!(warning.is_some(), "the failed write is reported");
        assert_eq!(
            meeting.segments.len(),
            1,
            "the transcript is still returned"
        );
    }

    // Error path: the first persist is what fails, so diarization must never
    // start — running it would burn minutes of native inference for a result
    // that has nowhere to go.
    #[tokio::test]
    async fn persist_transcript_then_diarize_skips_diarization_when_the_first_persist_fails() {
        let dir = tempfile::tempdir().unwrap();
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let flag = Arc::clone(&ran);
        let error = persist_transcript_then_diarize(
            dir.path().to_path_buf(),
            4_242,
            transcription(vec![segment(0, 1_000, "hello")]),
            Some(Box::pin(async move {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok((Ok(Vec::new()), None))
            })),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, AppError::Store(_)));
        assert!(
            !ran.load(std::sync::atomic::Ordering::SeqCst),
            "diarization must not run once persisting the transcript has failed"
        );
    }

    #[test]
    fn running_transcription_flag_matches_only_the_running_meeting_id() {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let running = Some((7, Arc::clone(&flag)));

        let found = running_transcription_flag(&running, 7).expect("flag for the running id");
        assert!(Arc::ptr_eq(&found, &flag));

        assert!(running_transcription_flag(&running, 8).is_none());
        assert!(running_transcription_flag(&None, 7).is_none());
    }

    #[test]
    fn cancel_guard_registers_and_clears_its_own_slot_on_drop() {
        let state = AppState::default();

        let (guard, flag) = TranscriptionCancelGuard::register(&state, 1);
        assert!(!flag.load(std::sync::atomic::Ordering::Relaxed));
        {
            let running = state.running_transcription.lock().unwrap();
            assert_eq!(running.as_ref().map(|(id, _)| *id), Some(1));
        }

        drop(guard);

        let running = state.running_transcription.lock().unwrap();
        assert!(running.is_none());
    }

    #[test]
    fn cancel_guard_drop_does_not_clear_a_different_runs_slot() {
        // Simulates run A's guard.drop() firing after run B has already
        // registered in its slot (e.g. a delayed unwind) — A's drop must not
        // clobber B's slot, or `cancel_transcription` could stop targeting
        // the run actually in flight.
        let state = AppState::default();
        let (guard_a, _flag_a) = TranscriptionCancelGuard::register(&state, 1);
        *state.running_transcription.lock().unwrap() =
            Some((2, Arc::new(std::sync::atomic::AtomicBool::new(false))));

        drop(guard_a);

        let running = state.running_transcription.lock().unwrap();
        assert_eq!(running.as_ref().map(|(id, _)| *id), Some(2));
    }
}
