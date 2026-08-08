//! WhisperPilot core: offline file transcription (language auto-detected) with
//! a summary to come.

pub mod audio;
pub mod diarize;
pub mod diarize_process;
pub mod error;
pub mod llm;
pub mod meetings;
pub mod models;
pub mod settings;
pub mod store;
pub mod streaming;
pub mod streaming_audio;
pub mod streaming_session;
pub mod streaming_store;
pub mod transcribe;

use error::{AppError, Result};
use meetings::{MeetingDto, MeetingSummaryDto};
use models::TaskModel;
use serde::{Deserialize, Serialize};
use settings::Settings;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use store::MeetingNotes;
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

/// Payload of the `transcription_progress` event (WP-58): whisper's own
/// 0-100 percent-complete figure for the transcription phase only — there is
/// no equivalent figure for the diarization phase that follows.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct TranscriptionProgressEvent {
    id: i64,
    percent: i32,
}

/// Emitted once per decoded Streaming window (`streaming_window`), whether
/// it succeeded or fail-open-skipped — `outcome_ok` distinguishes the two so
/// the UI can show "this span failed" rather than reading a skip as silence.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct StreamingWindowEvent {
    session_id: i64,
    window_index: i64,
    start_ms: i64,
    end_ms: i64,
    text: String,
    language: String,
    outcome_ok: bool,
}

/// Emitted once, right after a session starts (`streaming_sources`), naming
/// which capture source(s) actually came up — the mic-only-degradation
/// indicator WP-73's UI needs, since a silent fallback would be invisible.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct StreamingSourcesEvent {
    session_id: i64,
    mic: bool,
    system_audio: bool,
}

/// Emitted once the decode loop ends (`streaming_session_ended`) — either
/// because `stop_streaming_session` dropped the capture, or (not yet
/// possible in v1: no auto-timeout) it ended on its own.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct StreamingSessionEndedEvent {
    session_id: i64,
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

/// The whisper model is loaded lazily on first use and cached for the
/// session — loading ~800 MB should not block app launch.
///
/// `whisper_busy` enforces WP-71's mutual exclusion (Meeting transcription
/// vs. Streaming session, both contending for `model`'s one cached context)
/// via `streaming_session::WhisperUsageGuard`. It's a plain field, not
/// wrapped alongside `model`, so acquiring it stays synchronous without
/// holding `model`'s lock for the guard's whole lifetime.
#[derive(Default)]
struct AppState {
    model: Mutex<Option<Arc<WhisperContext>>>,
    whisper_busy: std::sync::atomic::AtomicU8,
    /// The meeting id and abort flag of the one Meeting transcription
    /// currently running, if any (WP-19). Only one can run at a time
    /// (`whisper_busy`), so a single slot — not a map — is enough.
    /// `cancel_transcription` flips the flag; `transcribe_meeting` clears
    /// this slot on every exit path via `TranscriptionCancelGuard`.
    running_transcription: std::sync::Mutex<Option<(i64, Arc<std::sync::atomic::AtomicBool>)>>,
    /// The running Streaming session's audio capture, present only while a
    /// session is active. Dropping it (via `stop_streaming_session` taking
    /// it out, or app shutdown dropping `AppState` itself) stops both
    /// capture streams, which cascades: the mixer thread ends, the sample
    /// channel disconnects, the decode loop ends, and the results-consuming
    /// task releases `whisper_busy` and marks the session stopped — see
    /// `docs/architecture.md`'s Streaming IPC section.
    #[cfg(target_os = "macos")]
    streaming_runtime: Mutex<Option<StreamingRuntime>>,
}

// Both fields are held for their effect, not read back: `session_id`
// documents which session this runtime belongs to (useful reading a debug
// dump or extending this later); `capture`'s only job is to exist until
// `stop_streaming_session` drops it, which is what actually stops capture.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
struct StreamingRuntime {
    session_id: i64,
    capture: streaming_audio::StreamingSession,
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
struct TranscriptionCancelGuard<'a> {
    state: &'a AppState,
    id: i64,
}

impl<'a> TranscriptionCancelGuard<'a> {
    fn register(state: &'a AppState, id: i64) -> (Self, Arc<std::sync::atomic::AtomicBool>) {
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
    let meeting = meetings::open_meeting(&app_support_dir, id)?;
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

/// Stop the meeting's in-flight transcription (WP-19): flips its abort flag,
/// which whisper's abort callback (or the pre-decode check in
/// `decode_and_transcribe`) turns into `AppError::Cancelled` — the run then
/// returns before any transcript is persisted, so no document is created.
#[tauri::command]
fn cancel_transcription(id: i64, state: State<'_, AppState>) -> Result<()> {
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

/// Auto-save a single transcript segment's edited text. `index` addresses the
/// meeting's currently displayed (speaker-coalesced) segment list, matching
/// what the workspace renders — see `meetings::update_segment`.
#[tauri::command]
fn update_segment(
    app: tauri::AppHandle,
    id: i64,
    index: usize,
    text: String,
) -> Result<MeetingDto> {
    meetings::update_segment(&app_data_dir(&app)?, id, index, text)
}

/// Auto-save the meeting notes fields as the user edits them.
#[tauri::command]
fn update_notes(app: tauri::AppHandle, notes: MeetingNotes) -> Result<MeetingDto> {
    meetings::update_notes(&app_data_dir(&app)?, notes)
}

/// List persisted Streaming sessions newest-touched first, for the
/// Streaming tab's session list.
#[tauri::command]
fn list_streaming_sessions(
    app: tauri::AppHandle,
) -> Result<Vec<streaming::StreamingSessionSummaryDto>> {
    streaming::list_streaming_sessions(&app_data_dir(&app)?)
}

/// Open a complete persisted Streaming session (all decoded windows) for
/// the Streaming workspace.
#[tauri::command]
fn open_streaming_session(
    app: tauri::AppHandle,
    id: i64,
) -> Result<streaming::StreamingSessionDto> {
    streaming::open_streaming_session(&app_data_dir(&app)?, id)
}

#[tauri::command]
fn rename_streaming_session(
    app: tauri::AppHandle,
    id: i64,
    title: String,
) -> Result<streaming::StreamingSessionDto> {
    streaming::rename_streaming_session(&app_data_dir(&app)?, id, title)
}

#[tauri::command]
fn delete_streaming_session(app: tauri::AppHandle, id: i64) -> Result<()> {
    streaming::delete_streaming_session(&app_data_dir(&app)?, id)
}

/// Runs on its own blocking thread for a session's whole lifetime: persists
/// each decoded window as it arrives (WP-72's incremental save) and emits
/// `streaming_window` so the UI updates live. When `results_rx` disconnects
/// — the decode loop ended because `stop_streaming_session` dropped the
/// capture — finalizes the session: marks it stopped, releases
/// `whisper_busy` so Meeting (or a new Streaming session) can run again, and
/// emits `streaming_session_ended`.
#[cfg(target_os = "macos")]
fn drive_streaming_results(
    app: tauri::AppHandle,
    app_support_dir: PathBuf,
    session_id: i64,
    results_rx: std::sync::mpsc::Receiver<streaming_session::WindowResult>,
) {
    for result in results_rx.iter() {
        let (text, language, outcome_ok) = match &result.outcome {
            Ok(transcription) => {
                let text = transcription
                    .segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                (text, transcription.language.clone(), true)
            }
            Err(e) => {
                log::warn!(
                    "streaming session {session_id} window {} fail-open: {e}",
                    result.window_index
                );
                (
                    String::new(),
                    transcribe::UNDETECTED_LANGUAGE.to_string(),
                    false,
                )
            }
        };
        let end_ms = result.start_ms + streaming_session::WINDOW_MS;

        match streaming_store::StreamingStore::open(&app_support_dir) {
            Ok(store) => {
                let window = streaming_store::NewStreamingWindow {
                    window_index: result.window_index as i64,
                    start_ms: result.start_ms as i64,
                    end_ms: end_ms as i64,
                    text: text.clone(),
                    language: language.clone(),
                    outcome_ok,
                };
                let now = now_ms().unwrap_or(result.start_ms as i64);
                if let Err(e) = store.append_window(session_id, &window, now) {
                    log::error!(
                        "streaming session {session_id}: failed to persist window {}: {e}",
                        result.window_index
                    );
                }
            }
            Err(e) => log::error!("streaming session {session_id}: failed to open store: {e}"),
        }

        let _ = app.emit(
            "streaming_window",
            StreamingWindowEvent {
                session_id,
                window_index: result.window_index as i64,
                start_ms: result.start_ms as i64,
                end_ms: end_ms as i64,
                text,
                language,
                outcome_ok,
            },
        );
    }

    // `results_rx.iter()` ended: the decode loop returned, which only
    // happens once the sample channel disconnects, which only happens once
    // the capture's mixer thread stops, which only happens once
    // `StreamingRuntime` (holding the capture) is dropped.
    let now = now_ms().unwrap_or(0);
    if let Ok(store) = streaming_store::StreamingStore::open(&app_support_dir) {
        if let Err(e) = store.mark_stopped(session_id, now) {
            log::error!("streaming session {session_id}: failed to mark stopped: {e}");
        }
    }
    streaming_session::release_whisper_busy(&app.state::<AppState>().whisper_busy);
    let _ = app.emit(
        "streaming_session_ended",
        StreamingSessionEndedEvent { session_id },
    );
}

/// Start a Streaming session: claims the shared Whisper context (mutually
/// exclusive with an active Meeting transcription, WP-71), creates the
/// session's DB record, starts audio capture (mic + system-audio, degrading
/// to whichever source(s) actually came up), and spawns the decode and
/// persistence loops. Returns once capture has started — decoding continues
/// in the background; the caller listens for `streaming_window` events.
#[cfg(target_os = "macos")]
#[tauri::command]
async fn start_streaming_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: Option<streaming_store::StreamingSessionId>,
) -> Result<streaming::StreamingSessionSummaryDto> {
    if state.streaming_runtime.lock().await.is_some() {
        return Err(AppError::Capture(
            "a Streaming session is already running".into(),
        ));
    }
    streaming_session::try_claim_streaming(&state.whisper_busy).map_err(|holder| {
        AppError::Capture(match holder {
            streaming_session::WhisperUser::Meeting => {
                "a meeting is currently transcribing; stop it before starting a Streaming session"
                    .to_string()
            }
            streaming_session::WhisperUser::Streaming => {
                "a Streaming session is already running".to_string()
            }
        })
    })?;

    let app_support_dir = app_data_dir(&app)?;

    let ctx = match state.model(app_support_dir.clone()).await {
        Ok(ctx) => ctx,
        Err(e) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            return Err(e);
        }
    };

    let now = now_ms()?;
    // A `session_id` continues an existing, previously-stopped session
    // ("Start" on an open past session resumes it rather than always
    // spawning a new one) — `starting_window_index` picks up window
    // numbering where that session left off; the fresh-session branch always
    // starts at 0.
    let (summary, starting_window_index) = match session_id {
        Some(id) => match streaming::resume_streaming_session(&app_support_dir, id, now) {
            Ok(result) => result,
            Err(e) => {
                streaming_session::release_whisper_busy(&state.whisper_busy);
                return Err(e);
            }
        },
        None => match streaming::create_streaming_session(&app_support_dir, now) {
            Ok(id) => (
                streaming::StreamingSessionSummaryDto {
                    id,
                    title: "New Streaming Session".to_string(),
                    created_at_ms: now,
                    updated_at_ms: now,
                    status: streaming_store::status::ACTIVE.to_string(),
                },
                0,
            ),
            Err(e) => {
                streaming_session::release_whisper_busy(&state.whisper_busy);
                return Err(e);
            }
        },
    };
    let session_id = summary.id;

    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    let capture = match streaming_audio::StreamingSession::start(samples_tx) {
        Ok(capture) => capture,
        Err(e) => {
            streaming_session::release_whisper_busy(&state.whisper_busy);
            // The session record already exists; leave it — an empty,
            // never-started session is a legitimate (if unfortunate) history
            // entry, consistent with a Meeting whose transcription failed.
            return Err(e);
        }
    };
    let active_sources = capture.active_sources();

    let (results_tx, results_rx) = std::sync::mpsc::channel();
    tokio::task::spawn_blocking(move || {
        streaming_session::run_windowed_decode(
            move || streaming_session::WhisperSessionDecoder::new(&ctx),
            samples_rx,
            results_tx,
            starting_window_index,
        )
    });

    let results_app = app.clone();
    let results_dir = app_support_dir.clone();
    tokio::task::spawn_blocking(move || {
        drive_streaming_results(results_app, results_dir, session_id, results_rx)
    });

    *state.streaming_runtime.lock().await = Some(StreamingRuntime {
        session_id,
        capture,
    });

    let _ = app.emit(
        "streaming_sources",
        StreamingSourcesEvent {
            session_id,
            mic: matches!(
                active_sources,
                streaming_audio::ActiveSources::Both | streaming_audio::ActiveSources::MicOnly
            ),
            system_audio: matches!(
                active_sources,
                streaming_audio::ActiveSources::Both
                    | streaming_audio::ActiveSources::SystemAudioOnly
            ),
        },
    );

    Ok(summary)
}

/// Stop the running Streaming session. Dropping the held capture stops both
/// audio sources; `drive_streaming_results` finishes persisting/emitting on
/// its own once the resulting sample/decode-loop disconnect cascades
/// through, releasing `whisper_busy` and marking the session stopped.
#[cfg(target_os = "macos")]
#[tauri::command]
async fn stop_streaming_session(state: State<'_, AppState>) -> Result<()> {
    let runtime = state.streaming_runtime.lock().await.take();
    if runtime.is_none() {
        return Err(AppError::Capture("no Streaming session is running".into()));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn start_streaming_session(
    _state: State<'_, AppState>,
    _session_id: Option<streaming_store::StreamingSessionId>,
) -> Result<streaming::StreamingSessionSummaryDto> {
    Err(AppError::Capture(
        "Streaming's audio capture is only available on macOS".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn stop_streaming_session(_state: State<'_, AppState>) -> Result<()> {
    Err(AppError::Capture(
        "Streaming's audio capture is only available on macOS".into(),
    ))
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

    let settings = settings::get_settings(&dir);
    if models::delete_clears_active_diarization_variant(&id, &settings.active_model_diarization) {
        settings::set_setting(&dir, "active_model.diarization", "none")?;
    }
    if settings.active_model_llm.as_deref() == Some(&id) {
        settings::set_setting(&dir, "active_model.llm", "")?;
    }
    Ok(())
}

/// Shared by both Craft/MFU commands (Meeting and Streaming). Errors name the
/// exact missing prerequisite rather than a generic failure.
fn resolve_llm_model_path(app_support_dir: &std::path::Path) -> Result<PathBuf> {
    let settings = settings::get_settings(app_support_dir);
    let llm_model_id = settings
        .active_model_llm
        .as_deref()
        .ok_or_else(|| AppError::Llm("no LLM model selected in Settings".into()))?;

    let model_path = models::primary_asset_path(app_support_dir, llm_model_id)
        .ok_or_else(|| AppError::Llm(format!("LLM model {llm_model_id} is not downloaded")))?;

    if !model_path.exists() {
        return Err(AppError::Llm(format!(
            "LLM model file not found at {}",
            model_path.display()
        )));
    }
    Ok(model_path)
}

/// Generate structured meeting notes (summary, decisions, action items, open
/// questions, participants) from the current transcript using the active LLM
/// model. Requires an LLM model to be downloaded and selected in Settings.
#[tauri::command]
async fn generate_notes(app: tauri::AppHandle, id: i64) -> Result<MeetingDto> {
    let app_support_dir = app_data_dir(&app)?;
    let model_path = resolve_llm_model_path(&app_support_dir)?;

    let store = store::Store::open(&app_support_dir)?;
    let _meeting = store
        .get_meeting(id)?
        .ok_or_else(|| AppError::Store(format!("meeting {id} was not found")))?;

    let segments = store.list_segments(id)?;
    if segments.is_empty() {
        return Err(AppError::Llm(
            "meeting has no transcript to summarize".into(),
        ));
    }

    let transcript: String = segments
        .iter()
        .map(|seg| {
            if let Some(sid) = seg.speaker_id {
                format!("Speaker {}: {}", sid, seg.text)
            } else {
                seg.text.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let app_support_dir_clone = app_support_dir.clone();
    let model_path_clone = model_path.clone();
    let transcript_clone = transcript.clone();
    let generated = tokio::task::spawn_blocking(move || {
        llm::generate_notes(&model_path_clone, &transcript_clone)
    })
    .await
    .map_err(|e| AppError::Llm(e.to_string()))??;

    store.upsert_notes(&store::MeetingNotes {
        meeting_id: id,
        summary: generated.summary,
        decisions: generated.decisions,
        action_items: generated.action_items,
        open_questions: generated.open_questions,
        participants: generated.participants,
    })?;

    meetings::open_meeting(&app_support_dir_clone, id)
}

/// Same local model/JSON contract as `generate_notes`, for a Streaming
/// session — `streaming::build_streaming_transcript` owns its guards.
#[tauri::command]
async fn generate_streaming_notes(
    app: tauri::AppHandle,
    id: i64,
) -> Result<streaming::StreamingSessionDto> {
    let app_support_dir = app_data_dir(&app)?;
    let model_path = resolve_llm_model_path(&app_support_dir)?;
    let transcript = streaming::build_streaming_transcript(&app_support_dir, id)?;

    let model_path_clone = model_path.clone();
    let transcript_clone = transcript.clone();
    let generated = tokio::task::spawn_blocking(move || {
        llm::generate_notes(&model_path_clone, &transcript_clone)
    })
    .await
    .map_err(|e| AppError::Llm(e.to_string()))??;

    let store = streaming_store::StreamingStore::open(&app_support_dir)?;
    store.upsert_notes(&streaming_store::StreamingNotes {
        session_id: id,
        summary: generated.summary,
        decisions: generated.decisions,
        action_items: generated.action_items,
        open_questions: generated.open_questions,
        participants: generated.participants,
    })?;

    streaming::open_streaming_session(&app_support_dir, id)
}

/// Returns the cleaned transcript without persisting it — the frontend
/// shows it as a diff for review; only `accept_streaming_prettify` writes it.
#[tauri::command]
async fn generate_streaming_prettify(app: tauri::AppHandle, id: i64) -> Result<String> {
    let app_support_dir = app_data_dir(&app)?;
    let model_path = resolve_llm_model_path(&app_support_dir)?;
    let transcript = streaming::build_streaming_transcript(&app_support_dir, id)?;

    tokio::task::spawn_blocking(move || llm::prettify_transcript(&model_path, &transcript))
        .await
        .map_err(|e| AppError::Llm(e.to_string()))?
}

#[tauri::command]
async fn accept_streaming_prettify(
    app: tauri::AppHandle,
    id: i64,
    text: String,
) -> Result<streaming::StreamingSessionDto> {
    let app_support_dir = app_data_dir(&app)?;
    let store = streaming_store::StreamingStore::open(&app_support_dir)?;
    store.upsert_prettified(id, &text)?;
    streaming::open_streaming_session(&app_support_dir, id)
}

/// Removes an accepted prettification so the raw per-window transcript is shown again.
#[tauri::command]
async fn revert_streaming_prettify(
    app: tauri::AppHandle,
    id: i64,
) -> Result<streaming::StreamingSessionDto> {
    let app_support_dir = app_data_dir(&app)?;
    let store = streaming_store::StreamingStore::open(&app_support_dir)?;
    store.delete_prettified(id)?;
    streaming::open_streaming_session(&app_support_dir, id)
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
            update_segment,
            update_notes,
            set_meeting_source,
            transcribe_meeting,
            cancel_transcription,
            list_streaming_sessions,
            open_streaming_session,
            rename_streaming_session,
            delete_streaming_session,
            start_streaming_session,
            stop_streaming_session,
            save_text_dialog,
            get_settings,
            set_setting,
            list_task_models,
            download_model,
            delete_model,
            generate_notes,
            generate_streaming_notes,
            generate_streaming_prettify,
            accept_streaming_prettify,
            revert_streaming_prettify
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

    #[test]
    fn transcription_progress_event_serializes_with_the_keys_the_frontend_expects() {
        let event = TranscriptionProgressEvent {
            id: 42,
            percent: 57,
        };

        let json = serde_json::to_value(&event).unwrap();

        assert_eq!(json["id"], serde_json::json!(42));
        assert_eq!(json["percent"], serde_json::json!(57));
        assert_eq!(
            json.as_object().unwrap().len(),
            2,
            "unexpected extra key in the transcription_progress payload"
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
