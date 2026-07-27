//! WhisperPilot core: offline file transcription (language auto-detected) with
//! a summary to come.

pub mod audio;
pub mod diarize;
pub mod diarize_process;
pub mod error;
pub mod meetings;
pub mod models;
pub mod settings;
pub mod store;
pub mod transcribe;

use error::{AppError, Result};
use meetings::{MeetingDto, MeetingSummaryDto};
use models::TaskModel;
use serde::{Deserialize, Serialize};
use settings::Settings;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager, State};
use tokio::sync::Mutex;
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

/// Payload of the `transcription_phase` event, emitted once a run moves from
/// transcribing into diarizing its samples. `phase` is a fixed literal today
/// (diarization is the only phase change the UI needs to know about beyond
/// the run simply being in flight) but stays a string, not a bool, so a
/// future phase can be added without changing the event's shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct TranscriptionPhaseEvent {
    id: i64,
    phase: &'static str,
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

fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Io(e.to_string()))
}

fn now_ms() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::Store(error.to_string()))
        .and_then(|duration| {
            i64::try_from(duration.as_millis())
                .map_err(|_| AppError::Store("current time exceeds i64 milliseconds".into()))
        })
}

/// The whisper model is loaded lazily on first use and cached for the session —
/// loading ~800 MB should not block app launch or fail startup when the model
/// is missing.
#[derive(Default)]
struct AppState {
    model: Mutex<Option<Arc<WhisperContext>>>,
}

impl AppState {
    async fn model(&self, app_support_dir: PathBuf) -> Result<Arc<WhisperContext>> {
        let mut guard = self.model.lock().await;
        if let Some(ctx) = guard.as_ref() {
            return Ok(Arc::clone(ctx));
        }
        // Loading is blocking and CPU-heavy; keep it off the async reactor.
        let ctx = tokio::task::spawn_blocking(move || transcribe::load_model(&app_support_dir))
            .await
            .map_err(|e| AppError::ModelLoad(e.to_string()))??;
        let ctx = Arc::new(ctx);
        *guard = Some(Arc::clone(&ctx));
        Ok(ctx)
    }
}

/// Open a native file picker for audio/video and return the chosen path.
#[tauri::command]
async fn open_file_dialog() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .add_filter(
                "Audio/Video",
                &[
                    "mp3", "wav", "m4a", "aac", "flac", "ogg", "opus", "mp4", "mov", "mkv", "webm",
                    "avi", "m4v",
                ],
            )
            .add_filter("All files", &["*"])
            .pick_file()
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .ok()
    .flatten()
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
async fn decode_and_transcribe(
    ctx: Arc<WhisperContext>,
    path: String,
) -> Result<(transcribe::Transcription, Vec<f32>)> {
    let input = PathBuf::from(&path);

    // Decode once (off the reactor); both transcription and diarization run
    // over the same samples.
    let samples = tokio::task::spawn_blocking(move || audio::load_samples(&input))
        .await
        .map_err(|e| AppError::Transcribe(e.to_string()))??;
    let transcription = {
        let samples = samples.clone();
        tokio::task::spawn_blocking(move || transcribe::transcribe(&ctx, &samples))
            .await
            .map_err(|e| AppError::Transcribe(e.to_string()))??
    };

    Ok((transcription, samples))
}

/// Persist `transcription` immediately, then run `diarization` (when a model is
/// active) and write the speaker ids it produces as a second, separate save.
///
/// Saving before diarization starts is load-bearing: diarization runs native
/// sherpa-onnx code that can abort the process outright, which no Rust error
/// path can catch, so anything unpersisted at that point is lost. See
/// `docs/architecture.md`'s Speaker Diarization section.
///
/// Any diarization failure degrades to the already-persisted speaker-less
/// segments and comes back as a warning, never failing the transcription.
async fn persist_transcript_then_diarize(
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

    let meeting = meetings::save_transcript(
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
    match meetings::save_transcript(
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

fn segment_dtos(segments: &[transcribe::Segment]) -> Vec<meetings::SegmentDto> {
    segments
        .iter()
        .map(|segment| meetings::SegmentDto {
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
fn set_meeting_source(app: tauri::AppHandle, id: i64, path: Option<String>) -> Result<MeetingDto> {
    meetings::set_meeting_source(&app_data_dir(&app)?, id, path)
}

/// Transcribe the meeting's attached source file into timestamped segments and
/// persist the result against the meeting. Whisper detects the language itself;
/// the meeting's stored `language` is an output of that decode, never an input
/// to it, so a value left by an earlier run does not influence this one.
#[tauri::command]
async fn transcribe_meeting(
    app: tauri::AppHandle,
    id: i64,
    state: State<'_, AppState>,
) -> Result<TranscribeMeetingResult> {
    let app_support_dir = app_data_dir(&app)?;
    let meeting = meetings::open_meeting(&app_support_dir, id)?;
    let path = meeting.source_path.ok_or_else(|| {
        AppError::Transcribe("meeting has no source file to transcribe".to_string())
    })?;

    let active_diarization_variant = diarization_variant_to_run(
        &settings::get_settings(&app_support_dir).active_model_diarization,
    )
    .map(str::to_string);

    let ctx = state.model(app_support_dir.clone()).await?;
    let (transcription, samples) = decode_and_transcribe(ctx, path).await?;

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

/// Save `content` to a user-chosen destination; return the path written.
#[tauri::command]
async fn save_text_dialog(content: String, default_name: Option<String>) -> Result<Option<String>> {
    let name = default_name.unwrap_or_else(|| "transcript.txt".to_string());
    let picked = tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_file_name(&name)
            .add_filter("Text", &["txt", "md"])
            .save_file()
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))?;

    let Some(dest) = picked else {
        return Ok(None);
    };
    std::fs::write(&dest, content)?;
    Ok(Some(dest.to_string_lossy().to_string()))
}

/// Create and return an empty persisted meeting. Attaching a file and starting
/// transcription are separate, explicit actions.
#[tauri::command]
fn create_meeting(app: tauri::AppHandle) -> Result<MeetingDto> {
    meetings::create_empty_meeting(&app_data_dir(&app)?, now_ms()?)
}

/// List persisted meetings newest first for the library sidebar.
#[tauri::command]
fn list_meetings(app: tauri::AppHandle) -> Result<Vec<MeetingSummaryDto>> {
    meetings::list_meetings(&app_data_dir(&app)?)
}

/// Open a complete persisted meeting for the active workspace.
#[tauri::command]
fn open_meeting(app: tauri::AppHandle, id: i64) -> Result<MeetingDto> {
    meetings::open_meeting(&app_data_dir(&app)?, id)
}

#[tauri::command]
fn rename_meeting(app: tauri::AppHandle, id: i64, title: String) -> Result<MeetingDto> {
    meetings::rename_meeting(&app_data_dir(&app)?, id, title)
}

#[tauri::command]
fn delete_meeting(app: tauri::AppHandle, id: i64) -> Result<()> {
    meetings::delete_meeting(&app_data_dir(&app)?, id)
}

/// Read all settings (theme, ui_language, active model), applying beta
/// defaults for any key never set.
#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<Settings> {
    let dir = app_data_dir(&app)?;
    Ok(settings::get_settings(&dir))
}

/// Update one known setting (theme, ui_language, or active_model.transcription)
/// and persist it immediately; rejects an unknown key or an invalid value.
#[tauri::command]
fn set_setting(app: tauri::AppHandle, key: String, value: String) -> Result<Settings> {
    let dir = app_data_dir(&app)?;
    settings::set_setting(&dir, &key, &value)
}

/// List the AI models catalog (transcription, diarization) with each
/// entry's current downloaded state.
#[tauri::command]
fn list_task_models(app: tauri::AppHandle) -> Result<Vec<TaskModel>> {
    let dir = app_data_dir(&app)?;
    Ok(models::list_task_models(&dir))
}

/// Download the catalog entry `id`, verifying SHA-256 before marking it
/// ready. Emits `model_download_progress { id, fraction, stage }` as bytes
/// arrive and again when the fetched bytes move on to hash verification.
#[tauri::command]
async fn download_model(app: tauri::AppHandle, id: String) -> Result<()> {
    let dir = app_data_dir(&app)?;
    let progress_app = app.clone();
    let progress_id = id.clone();
    models::download_model(&dir, &id, move |fraction, stage| {
        let _ = progress_app.emit(
            "model_download_progress",
            serde_json::json!({ "id": progress_id, "fraction": fraction, "stage": stage }),
        );
    })
    .await
}

/// Delete catalog entry `id`'s downloaded file(s), returning it to
/// not-downloaded. If `id` was the currently active diarization variant,
/// also reverts `active_model.diarization` to "none" so a later
/// transcription does not fail open against a model no longer on disk.
#[tauri::command]
fn delete_model(app: tauri::AppHandle, id: String) -> Result<()> {
    let dir = app_data_dir(&app)?;
    models::delete_model(&dir, &id)?;

    let active = settings::get_settings(&dir).active_model_diarization;
    if models::delete_clears_active_diarization_variant(&id, &active) {
        settings::set_setting(&dir, "active_model.diarization", "none")?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // This same binary is re-executed as a diarization worker (WP-53). That
    // launch must be recognized before anything else starts: it builds no
    // window and touches no Tauri state.
    if let Some(code) = diarize_process::worker_exit_code() {
        std::process::exit(code);
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            open_file_dialog,
            create_meeting,
            list_meetings,
            open_meeting,
            rename_meeting,
            delete_meeting,
            set_meeting_source,
            transcribe_meeting,
            save_text_dialog,
            get_settings,
            set_setting,
            list_task_models,
            download_model,
            delete_model
        ])
        .run(tauri::generate_context!())
        .expect("error while running WhisperPilot");
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
        meetings::create_empty_meeting(dir, 0).unwrap().id
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
                *seen.lock().unwrap() = Some(meetings::open_meeting(&path, id).unwrap());
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
        let reopened = meetings::open_meeting(dir.path(), id).unwrap();
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
        let reopened = meetings::open_meeting(dir.path(), id).unwrap();
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
        let reopened = meetings::open_meeting(dir.path(), id).unwrap();
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
                meetings::delete_meeting(&path, id).unwrap();
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

    // Pins the `transcription_phase` event's wire shape against the
    // frontend's `TranscriptionPhase` interface in `src/ipc.ts` (`{ id:
    // number; phase: "diarizing" }`) so a field rename on either side fails
    // this test instead of silently leaving the frontend's id/phase guard in
    // `App.tsx` unable to match the event.
    #[test]
    fn transcription_phase_event_serializes_with_the_keys_and_casing_the_frontend_expects() {
        let event = TranscriptionPhaseEvent {
            id: 42,
            phase: "diarizing",
        };

        let json = serde_json::to_value(&event).unwrap();

        assert_eq!(json["id"], serde_json::json!(42));
        assert_eq!(json["phase"], serde_json::json!("diarizing"));
        assert_eq!(
            json.as_object().unwrap().len(),
            2,
            "unexpected extra key in the transcription_phase payload"
        );
    }
}
