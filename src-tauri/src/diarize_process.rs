//! Process isolation for the native diarization engine — see ADR-013 and
//! docs/architecture.md's Diarization Process Isolation section.
//!
//! The child is *this same binary* re-executed with a hidden argv mode
//! (`WORKER_FLAG`); each attempt is independent, so a caller may run more
//! than one per transcription (WP-57's fallback does). macOS only
//! (`std::os::unix::process::ExitStatusExt`).

use crate::diarize::{self, SpeakerTurn};
use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Hidden argv flag that turns a normal app launch into a diarization worker.
/// Not a user-facing CLI: `prepare_and_supervise` is its only source.
pub const WORKER_FLAG: &str = "--wp-diarize-worker";

/// How long the child may go silent before it is presumed hung and killed.
///
/// An *inactivity* budget, not a total one, so it does not scale with
/// recording length during the embedding loop. It must still absorb two
/// genuinely silent stretches: sherpa-onnx segments the whole file before its
/// first progress callback, and clusters after the last one.
pub const DEFAULT_INACTIVITY: Duration = Duration::from_secs(120);

/// How often the supervisor re-checks a running child. Small enough that a
/// finished child is reaped promptly, large enough not to spin.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// What `Child::kill` sends, and therefore the one signal that means "we did
/// this" rather than "the engine faulted".
const SIGKILL: i32 = 9;

/// Exit codes the worker uses. The parent only needs "non-zero means the
/// engine reported a real error"; these exist so a log or a bug report can say
/// *which* error without parsing stderr.
const EXIT_BAD_REQUEST: i32 = 2;
const EXIT_ENGINE_ERROR: i32 = 3;
const EXIT_UNWRITABLE_RESULT: i32 = 4;
const EXIT_PARENT_GONE: i32 = 5;

/// How long a transport file may sit in the cache dir before it is presumed
/// abandoned. Generous enough that no in-flight run is ever swept.
const STALE_TRANSPORT_AGE: Duration = Duration::from_secs(6 * 60 * 60);

/// The file-name prefixes this module creates, and the only ones it deletes.
const TRANSPORT_PREFIXES: [&str; 3] = ["samples-", "turns-", "request-"];

/// What became of one diarization child.
///
/// The four cases stay separate all the way to the caller because a recovery
/// layer keys off exactly this distinction — WP-57 retries a crash but not a
/// timeout. Do not collapse them into one error (ADR-013).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildOutcome {
    /// Clean exit *and* a readable result payload.
    Completed(Vec<SpeakerTurn>),
    /// Killed by a signal — the native fault this whole module exists for.
    Crashed { signal: i32 },
    /// The engine reported an ordinary error, or exited cleanly without a
    /// usable payload. `code` is `None` when no child ran at all — a setup
    /// failure such as an unwritable cache dir — which is not the same as the
    /// engine failing, and callers that retry should treat it differently.
    Failed { code: Option<i32>, detail: String },
    /// Stopped making progress and was killed on the inactivity budget.
    TimedOut,
}

impl ChildOutcome {
    /// Collapse to the `Result` the existing fail-open path already handles.
    ///
    /// Every failure becomes an `AppError::Diarization`, so
    /// `apply_diarization_outcome` degrades the run to speaker-less segments
    /// exactly as it does for an in-process engine error. The messages stay
    /// distinct per case; callers that need to *branch* on the case should
    /// match `ChildOutcome` itself rather than parse these strings.
    pub fn into_result(self) -> Result<Vec<SpeakerTurn>> {
        match self {
            ChildOutcome::Completed(turns) => Ok(turns),
            ChildOutcome::Crashed { signal } => Err(AppError::Diarization(format!(
                "the speaker-identification engine crashed on this recording (signal {signal})"
            ))),
            ChildOutcome::TimedOut => Err(AppError::Diarization(
                "the speaker-identification engine stopped responding and was stopped".to_string(),
            )),
            ChildOutcome::Failed { code, detail } => Err(AppError::Diarization(match code {
                Some(code) => {
                    format!("the speaker-identification engine failed (exit {code}): {detail}")
                }
                None => format!("the speaker-identification engine failed: {detail}"),
            })),
        }
    }
}

/// One supervised child run. `pid` is reported so a caller (or a test) can
/// confirm the process is gone rather than orphaned.
#[derive(Debug)]
pub struct SupervisedRun {
    pub pid: u32,
    pub outcome: ChildOutcome,
}

/// Everything the worker needs, handed over as a small JSON file rather than
/// argv so paths with spaces and a growing field set stay uncomplicated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerRequest {
    pub app_support_dir: PathBuf,
    pub samples_path: PathBuf,
    pub output_path: PathBuf,
    pub variant: String,
    pub speaker_count: Option<i32>,
}

/// Extract the worker request path from a process's argv, or `None` for a
/// normal app launch. Kept separate from `std::env::args()` so the argv
/// contract is testable without spawning anything.
pub fn worker_request_path(args: &[String]) -> Option<PathBuf> {
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        if arg == WORKER_FLAG {
            return rest.next().map(PathBuf::from);
        }
    }
    None
}

/// Run the engine if this process was launched as a diarization worker,
/// returning the exit code it should terminate with. `None` means an ordinary
/// app launch, so `run()` continues into Tauri. Returns the code rather than
/// exiting so the decision stays in one place.
pub fn worker_exit_code() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    let request_path = worker_request_path(&args)?;
    Some(execute_worker(&request_path))
}

/// The worker body: read the request, run the real in-process engine, write
/// the turns as JSON. Returns the process exit code.
fn execute_worker(request_path: &Path) -> i32 {
    // Armed before any other work: quitting the app mid-run must not leave
    // inference running. Stdin is the parent's pipe, so its close is the one
    // death signal that arrives however the parent went — including SIGKILL.
    std::thread::spawn(|| {
        wait_for_parent_to_go(std::io::stdin());
        // `_exit`, not `exit`: the main thread is inside native inference, so
        // running onnxruntime's static destructors underneath it could hang
        // exactly the process this is trying to reap. Not `abort` either —
        // SIGABRT would leave a macOS crash report behind every ordinary quit.
        unsafe { libc::_exit(EXIT_PARENT_GONE) };
    });

    let request = match std::fs::read(request_path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<WorkerRequest>(&bytes).map_err(|e| e.to_string())
        }) {
        Ok(request) => request,
        Err(e) => {
            eprintln!(
                "diarization worker: unusable request at {}: {e}",
                request_path.display()
            );
            return EXIT_BAD_REQUEST;
        }
    };

    let samples = match read_samples(&request.samples_path) {
        Ok(samples) => samples,
        Err(e) => {
            eprintln!("diarization worker: {e}");
            return EXIT_BAD_REQUEST;
        }
    };

    // Anchors the parent's inactivity clock before model loading begins, so a
    // slow load is never mistaken for a hang.
    report_progress(0, 0);

    let progress: diarize::ProgressCallback = Box::new(|done, total| {
        report_progress(done, total);
        0
    });

    match diarize::diarize_samples_with_progress(
        &request.app_support_dir,
        samples,
        request.speaker_count,
        &request.variant,
        Some(progress),
    ) {
        Ok(turns) => match serde_json::to_vec(&turns)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                std::fs::write(&request.output_path, bytes).map_err(|e| e.to_string())
            }) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("diarization worker: could not write the result: {e}");
                EXIT_UNWRITABLE_RESULT
            }
        },
        Err(e) => {
            eprintln!("diarization worker: {e}");
            EXIT_ENGINE_ERROR
        }
    }
}

/// Block until the parent's end of `reader` closes. Anything the parent sends
/// is discarded — this channel carries no data, only the fact of its own end.
fn wait_for_parent_to_go(reader: impl Read) {
    let mut reader = BufReader::new(reader);
    let mut discard = Vec::new();
    loop {
        discard.clear();
        match reader.read_until(b'\n', &mut discard) {
            Ok(0) => return,
            Ok(_) => {}
            // A signal interrupting the read is not the parent going away.
            // Ending the run here would abandon live inference.
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// Delete transport files older than `max_age`.
///
/// The supervised paths clean up after themselves; this covers the one that
/// cannot — the parent being killed outright, which leaves its ~55MB sample
/// file with no owner. Age-based rather than pid-based so a run still in
/// flight in another process is never swept out from under itself.
fn sweep_stale_transports(work_dir: &Path, max_age: Duration) {
    let Ok(entries) = std::fs::read_dir(work_dir) else {
        return;
    };
    for entry in entries.flatten() {
        // Only this module's own transports: age alone is too blunt a licence
        // to delete from a shared directory.
        let ours = entry
            .file_name()
            .to_str()
            .is_some_and(|name| TRANSPORT_PREFIXES.iter().any(|p| name.starts_with(p)));
        if !ours {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .map(|modified| modified.elapsed().unwrap_or_default() >= max_age)
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// One heartbeat line on stdout. Rust's stdout is line-buffered, so the
/// newline is what actually reaches the parent's pipe.
fn report_progress(done: i32, total: i32) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "progress {done} {total}");
}

/// Run diarization in a child process, re-executing this binary.
///
/// Returns the raw `ChildOutcome` rather than a `Result` so a caller can tell a
/// native crash from a timeout from an engine error; use
/// [`ChildOutcome::into_result`] when the existing fail-open path is all that
/// is wanted.
pub fn diarize_isolated(
    app_support_dir: &Path,
    samples: Vec<f32>,
    speaker_count: Option<i32>,
    active_variant: &str,
) -> ChildOutcome {
    let worker_exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            return ChildOutcome::Failed {
                code: None,
                detail: format!("could not locate this executable to isolate diarization: {e}"),
            }
        }
    };
    diarize_isolated_with(
        &worker_exe,
        app_support_dir,
        samples,
        speaker_count,
        active_variant,
        DEFAULT_INACTIVITY,
    )
}

/// [`diarize_isolated`] with the worker binary and inactivity budget supplied
/// explicitly. Exists because `current_exe()` under `cargo test` is the test
/// binary, not the app, so the integration regression test must name the real
/// one via `CARGO_BIN_EXE_*`.
pub fn diarize_isolated_with(
    worker_exe: &Path,
    app_support_dir: &Path,
    samples: Vec<f32>,
    speaker_count: Option<i32>,
    active_variant: &str,
    inactivity: Duration,
) -> ChildOutcome {
    let work_dir = app_support_dir.join("cache").join("diarize");
    if let Err(e) = std::fs::create_dir_all(&work_dir) {
        return ChildOutcome::Failed {
            code: None,
            detail: format!("could not prepare {}: {e}", work_dir.display()),
        };
    }
    sweep_stale_transports(&work_dir, STALE_TRANSPORT_AGE);

    // Unique per attempt, not just per process: WP-57 runs a second attempt
    // inside the same process after the first one fails.
    static ATTEMPT: AtomicU64 = AtomicU64::new(0);
    let tag = format!(
        "{}-{}",
        std::process::id(),
        ATTEMPT.fetch_add(1, Ordering::Relaxed)
    );
    let samples_path = work_dir.join(format!("samples-{tag}.f32"));
    let output_path = work_dir.join(format!("turns-{tag}.json"));
    let request_path = work_dir.join(format!("request-{tag}.json"));

    let request = WorkerRequest {
        app_support_dir: app_support_dir.to_path_buf(),
        samples_path,
        output_path,
        variant: active_variant.to_string(),
        speaker_count,
    };
    let outcome = prepare_and_supervise(worker_exe, &request, &request_path, samples, inactivity);
    let WorkerRequest {
        samples_path,
        output_path,
        ..
    } = request;

    // Best-effort: a leftover sample file is harmless but can be ~55MB, so it
    // is worth removing even when the child died badly.
    for path in [&samples_path, &output_path, &request_path] {
        let _ = std::fs::remove_file(path);
    }

    outcome
}

fn prepare_and_supervise(
    worker_exe: &Path,
    request: &WorkerRequest,
    request_path: &Path,
    samples: Vec<f32>,
    inactivity: Duration,
) -> ChildOutcome {
    if let Err(e) = write_samples(&request.samples_path, &samples) {
        return ChildOutcome::Failed {
            code: None,
            detail: e.to_string(),
        };
    }
    // Freed here rather than at the end of the call: the child owns a full
    // second copy of these samples for the whole run.
    drop(samples);

    let request_written = serde_json::to_vec(request)
        .map_err(|e| e.to_string())
        .and_then(|bytes| std::fs::write(request_path, bytes).map_err(|e| e.to_string()));
    if let Err(e) = request_written {
        return ChildOutcome::Failed {
            code: None,
            detail: format!("could not write the diarization request: {e}"),
        };
    }

    let mut command = Command::new(worker_exe);
    command.arg(WORKER_FLAG).arg(request_path);

    // The binary's own LC_RPATH (WP-60) is what normally resolves
    // @rpath/libonnxruntime and @rpath/libsherpa-onnx-c-api. Keep setting the
    // fallback explicitly anyway, so the child does not depend on how the
    // parent happened to be started, or on the layout it was launched from.
    let exe_dir = worker_exe.parent().unwrap_or_else(|| Path::new("."));
    command.env(
        "DYLD_FALLBACK_LIBRARY_PATH",
        dyld_fallback_library_path(
            exe_dir,
            std::env::var_os("DYLD_FALLBACK_LIBRARY_PATH").as_deref(),
        ),
    );

    match supervise(&mut command, &request.output_path, inactivity) {
        Ok(run) => run.outcome,
        Err(e) => ChildOutcome::Failed {
            code: None,
            detail: e.to_string(),
        },
    }
}

/// Build the child's dylib search path: its own directory first, then the
/// `.app` bundle's `Contents/Frameworks` sibling, then whatever this process
/// inherited. Guarantees only that the child is no worse off than the parent;
/// the packaging caveats are in ADR-013.
fn dyld_fallback_library_path(exe_dir: &Path, inherited: Option<&OsStr>) -> OsString {
    let mut entries: Vec<OsString> = vec![exe_dir.as_os_str().to_os_string()];
    if let Some(parent) = exe_dir.parent() {
        entries.push(parent.join("Frameworks").into_os_string());
    }
    if let Some(inherited) = inherited.filter(|value| !value.is_empty()) {
        entries.push(inherited.to_os_string());
    }

    let mut value = OsString::new();
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            value.push(":");
        }
        value.push(entry);
    }
    value
}

/// Spawn `command` and watch it until it finishes or goes quiet.
///
/// The supervisor owns the child's stdio: stdout carries the heartbeat that
/// resets the inactivity clock, and stderr is drained on its own thread so a
/// chatty child can never fill the pipe, block, and look hung.
///
/// `Err` means the child could not be started at all; every other ending —
/// including a kill — is a `ChildOutcome`.
fn supervise(
    command: &mut Command,
    output_path: &Path,
    inactivity: Duration,
) -> Result<SupervisedRun> {
    let mut child = command
        // Piped, never null: the child watches this handle to learn that the
        // app quit, so it must be something whose close it can observe.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            AppError::Diarization(format!(
                "could not start the speaker-identification process: {e}"
            ))
        })?;
    let pid = child.id();

    let last_activity = Arc::new(Mutex::new(Instant::now()));
    let heartbeat = child.stdout.take().map(|stdout| {
        let last_activity = Arc::clone(&last_activity);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if line.is_err() {
                    break;
                }
                *last_activity.lock().expect("heartbeat clock not poisoned") = Instant::now();
            }
        })
    });
    let stderr_drain = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut text);
            text
        })
    });

    let (exit_status, killed) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (Some(status), false),
            Ok(None) => {}
            Err(e) => {
                return Err(AppError::Diarization(format!(
                    "lost track of the speaker-identification process: {e}"
                )))
            }
        }

        let idle = last_activity
            .lock()
            .expect("heartbeat clock not poisoned")
            .elapsed();
        if idle >= inactivity {
            let _ = child.kill();
            // Reap it. Without this the killed child lingers as a zombie
            // holding its share of the model files. The status is kept rather
            // than discarded: the child may have finished in the moment
            // between the last poll and this kill, and reporting a completed
            // run as a hang would lose real speakers.
            break (child.wait().ok(), true);
        }

        std::thread::sleep(POLL_INTERVAL);
    };

    if let Some(heartbeat) = heartbeat {
        let _ = heartbeat.join();
    }
    let stderr = stderr_drain
        .and_then(|drain| drain.join().ok())
        .unwrap_or_default();

    let outcome = if killed {
        outcome_after_kill(exit_status, output_path, stderr)
    } else {
        match exit_status {
            Some(status) => classify_exit(status, output_path, stderr),
            None => ChildOutcome::Failed {
                code: None,
                detail: "the speaker-identification process ended without a status".to_string(),
            },
        }
    };

    Ok(SupervisedRun { pid, outcome })
}

/// Decide what a killed child's run amounted to. `reaped` is the status
/// `wait` returned after the kill: `Some` means it had already finished on its
/// own and that real result wins, `None` means it was genuinely hung.
fn outcome_after_kill(
    reaped: Option<ExitStatus>,
    output_path: &Path,
    stderr: String,
) -> ChildOutcome {
    match reaped {
        Some(status) => match classify_exit(status, output_path, stderr) {
            // Only SIGKILL, which is this supervisor's own doing. Any other
            // signal is a real fault the child hit in the same window — and
            // reporting that as a hang would hide the one outcome a recovery
            // layer retries on.
            ChildOutcome::Crashed { signal } if signal == SIGKILL => ChildOutcome::TimedOut,
            other => other,
        },
        None => ChildOutcome::TimedOut,
    }
}

/// Map a finished child's exit status onto an outcome. A clean exit is only a
/// success when it also left a readable payload — an engine that returns 0 and
/// writes nothing has not produced speakers.
fn classify_exit(status: ExitStatus, output_path: &Path, stderr: String) -> ChildOutcome {
    if let Some(signal) = status.signal() {
        return ChildOutcome::Crashed { signal };
    }

    let code = status.code();
    if code != Some(0) {
        return ChildOutcome::Failed {
            code,
            detail: stderr.trim().to_string(),
        };
    }

    match read_turns(output_path) {
        Ok(turns) => ChildOutcome::Completed(turns),
        Err(e) => ChildOutcome::Failed {
            code: Some(0),
            detail: format!("{e}"),
        },
    }
}

fn read_turns(path: &Path) -> Result<Vec<SpeakerTurn>> {
    let bytes = std::fs::read(path).map_err(|e| {
        AppError::Diarization(format!(
            "the engine exited cleanly but left no result at {}: {e}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Diarization(format!("the engine's result was unreadable: {e}")))
}

/// Samples cross the process boundary as a raw little-endian `f32` file rather
/// than a pipe — see ADR-013 for why.
fn write_samples(path: &Path, samples: &[f32]) -> Result<()> {
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).map_err(|e| {
        AppError::Diarization(format!(
            "could not stage audio for the speaker-identification process at {}: {e}",
            path.display()
        ))
    })
}

fn read_samples(path: &Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path).map_err(|e| {
        AppError::Diarization(format!(
            "could not read staged audio at {}: {e}",
            path.display()
        ))
    })?;
    if bytes.len() % 4 != 0 {
        // `chunks_exact` would drop the partial tail silently, diarizing a
        // slightly different recording than the one transcribed.
        return Err(AppError::Diarization(format!(
            "staged audio at {} is truncated ({} bytes is not a whole number of samples)",
            path.display(),
            bytes.len()
        )));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

// --- Fallback: one-hop retry on crash (WP-57) ---------------------------

/// Return the other known embedding variant, or `None` when `active` is
/// not a recognised variant (no fallback possible).
fn fallback_variant_for(active: &str) -> Option<&'static str> {
    match active {
        "titanet-large" => Some("campplus"),
        "campplus" => Some("titanet-large"),
        _ => None,
    }
}

/// A crash is the only child outcome worth retrying: it is deterministic per
/// input + model, so the same model would just crash again, but the *other*
/// model may survive. A timeout means the time budget is spent; an engine
/// error is a real failure — neither justifies a retry.
fn should_retry_diarization(outcome: &ChildOutcome) -> bool {
    matches!(outcome, ChildOutcome::Crashed { .. })
}

/// Human-readable label for a variant id so diarization warnings name the
/// model the user sees in Settings, not the internal id.
fn variant_display_name(variant: &str) -> &str {
    match variant {
        "titanet-large" => "TitaNet-large",
        "campplus" => "CAM++",
        other => other,
    }
}

/// Run diarization isolated, retrying once with the other embedding model on
/// a native crash. Returns the turns plus an optional warning — `None` on a
/// clean first-attempt success, `Some` when the fallback was used (successful
/// or not).
///
/// The fallback attempt itself runs under the same process isolation as the
/// first. `samples` are cloned so the retry has its own copy; on a typical
/// recording (~6 MB) this is acceptable even on the common non-crash path.
pub fn diarize_with_fallback(
    app_support_dir: &Path,
    samples: Vec<f32>,
    speaker_count: Option<i32>,
    active_variant: &str,
) -> (Result<Vec<SpeakerTurn>>, Option<String>) {
    let first = diarize_isolated(
        app_support_dir,
        samples.clone(),
        speaker_count,
        active_variant,
    );

    if !should_retry_diarization(&first) {
        return (first.into_result(), None);
    }

    let fallback_variant = match fallback_variant_for(active_variant) {
        Some(v) => v,
        None => return (first.into_result(), None),
    };

    if !crate::models::is_diarization_variant_downloaded(app_support_dir, fallback_variant) {
        return (first.into_result(), None);
    }

    let second = diarize_isolated(app_support_dir, samples, speaker_count, fallback_variant);

    match second {
        ChildOutcome::Completed(turns) => {
            let warning = format!(
                "used {} because {} failed on this recording",
                variant_display_name(fallback_variant),
                variant_display_name(active_variant),
            );
            (Ok(turns), Some(warning))
        }
        other => (other.into_result(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// A child that is gone from the process table. Used to prove the
    /// supervisor reaps what it kills rather than leaking an orphan holding
    /// the diarization models open.
    fn process_is_gone(pid: u32) -> bool {
        !Command::new("ps")
            .arg("-p")
            .arg(pid.to_string())
            .output()
            .expect("ps must be available on macOS")
            .status
            .success()
    }

    fn turns_json(turns: &[(u32, u32, i32)]) -> String {
        let items: Vec<String> = turns
            .iter()
            .map(|(start_ms, end_ms, speaker)| {
                format!(r#"{{"start_ms":{start_ms},"end_ms":{end_ms},"speaker":{speaker}}}"#)
            })
            .collect();
        format!("[{}]", items.join(","))
    }

    /// Build a `sh -c` child. Every supervised child must have its pipes set
    /// by the supervisor itself, so nothing here configures stdio.
    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        command
    }

    fn out_path(dir: &Path) -> std::path::PathBuf {
        dir.join("turns.json")
    }

    // --- DoD C-5: the four child outcomes, each distinguished -------------

    #[test]
    fn supervise_reports_completed_when_the_child_exits_cleanly_with_a_payload() {
        let dir = tempfile::tempdir().unwrap();
        let output = out_path(dir.path());
        let payload = turns_json(&[(0, 1_500, 0), (1_500, 4_000, 1)]);
        let mut command = shell(&format!("printf '%s' '{payload}' > {}", output.display()));

        let run = supervise(&mut command, &output, Duration::from_secs(10)).unwrap();

        assert_eq!(
            run.outcome,
            ChildOutcome::Completed(vec![
                crate::diarize::SpeakerTurn {
                    start_ms: 0,
                    end_ms: 1_500,
                    speaker: 0
                },
                crate::diarize::SpeakerTurn {
                    start_ms: 1_500,
                    end_ms: 4_000,
                    speaker: 1
                },
            ])
        );
    }

    #[test]
    fn supervise_reports_crashed_when_the_child_dies_by_signal() {
        let dir = tempfile::tempdir().unwrap();
        let output = out_path(dir.path());
        // SIGBUS (10 on macOS) is the exact signal sherpa-onnx's native
        // clustering raises on the WP-53 reproducer.
        let mut command = shell("kill -BUS $$; sleep 5");

        let run = supervise(&mut command, &output, Duration::from_secs(10)).unwrap();

        assert_eq!(run.outcome, ChildOutcome::Crashed { signal: 10 });
    }

    #[test]
    fn supervise_reports_failed_with_detail_when_the_child_exits_non_zero() {
        let dir = tempfile::tempdir().unwrap();
        let output = out_path(dir.path());
        let mut command = shell("echo 'embedding model missing' >&2; exit 3");

        let run = supervise(&mut command, &output, Duration::from_secs(10)).unwrap();

        match run.outcome {
            ChildOutcome::Failed { code, detail } => {
                assert_eq!(code, Some(3));
                assert!(
                    detail.contains("embedding model missing"),
                    "the child's own stderr must survive into the outcome, got: {detail}"
                );
            }
            other => panic!("expected a non-zero-exit failure, got {other:?}"),
        }
    }

    #[test]
    fn supervise_reports_timed_out_and_reaps_an_inactive_child() {
        let dir = tempfile::tempdir().unwrap();
        let output = out_path(dir.path());
        // Never writes a heartbeat, so the inactivity window is the only
        // thing that can end this run.
        let mut command = shell("sleep 30");

        let run = supervise(&mut command, &output, Duration::from_millis(200)).unwrap();

        assert_eq!(run.outcome, ChildOutcome::TimedOut);
        assert!(
            process_is_gone(run.pid),
            "the killed child must be reaped, not left orphaned holding the models"
        );
    }

    // --- DoD C-5: a clean exit is not success on its own ------------------

    #[test]
    fn supervise_reports_failed_when_a_clean_exit_leaves_no_readable_payload() {
        let dir = tempfile::tempdir().unwrap();
        let output = out_path(dir.path());
        let mut command = shell("exit 0");

        let run = supervise(&mut command, &output, Duration::from_secs(10)).unwrap();

        match run.outcome {
            ChildOutcome::Failed { code, .. } => assert_eq!(code, Some(0)),
            other => panic!("a clean exit with no payload is not success, got {other:?}"),
        }
    }

    // --- DoD C-4: inactivity, not total wall-clock ------------------------

    #[test]
    fn supervise_does_not_time_out_a_child_that_keeps_reporting_progress() {
        let dir = tempfile::tempdir().unwrap();
        let output = out_path(dir.path());
        let payload = turns_json(&[(0, 500, 0)]);
        // Runs ~1.8s in total — well past the 1s window — but never goes
        // quiet for more than ~150ms. A total wall-clock timeout would kill
        // this; an inactivity timeout must not.
        let mut command = shell(&format!(
            "for i in 1 2 3 4 5 6 7 8 9 10 11 12; do echo progress; sleep 0.15; done; \
             printf '%s' '{}' > {}",
            payload,
            output.display()
        ));

        let started = Instant::now();
        // Use a 2-second inactivity window so a child briefly stalled by
        // parallel test load is not spuriously killed (observed flakiness
        // at 1s). The total wall-clock runtime (~1.8s) still exceeds 1s,
        // proving the inactivity-vs-wall-clock distinction.
        let run = supervise(&mut command, &output, Duration::from_secs(2)).unwrap();

        assert!(
            matches!(run.outcome, ChildOutcome::Completed(_)),
            "a child that keeps reporting progress must survive, got {:?}",
            run.outcome
        );
        assert!(
            started.elapsed() > Duration::from_secs(1),
            "this test only proves inactivity-vs-wall-clock if the run outlived the 1-second wall clock"
        );
    }

    // --- DoD C-6: explicit dylib resolution -------------------------------

    #[test]
    fn dyld_fallback_library_path_leads_with_the_executable_and_bundle_directories() {
        let value =
            dyld_fallback_library_path(Path::new("/Apps/WhisperPilot.app/Contents/MacOS"), None);
        let value = value.to_string_lossy();
        let entries: Vec<&str> = value.split(':').collect();

        assert_eq!(entries[0], "/Apps/WhisperPilot.app/Contents/MacOS");
        assert_eq!(entries[1], "/Apps/WhisperPilot.app/Contents/Frameworks");
    }

    #[test]
    fn dyld_fallback_library_path_preserves_an_inherited_value_after_its_own() {
        let value = dyld_fallback_library_path(
            Path::new("/build/debug"),
            Some(std::ffi::OsStr::new("/inherited/one:/inherited/two")),
        );
        let value = value.to_string_lossy();

        assert!(
            value.ends_with("/inherited/one:/inherited/two"),
            "an inherited search path must be kept, not replaced: {value}"
        );
        assert!(
            value.starts_with("/build/debug:"),
            "the child's own executable directory must still win: {value}"
        );
    }

    // --- The hidden argv mode --------------------------------------------

    #[test]
    fn worker_request_path_is_recognized_in_the_hidden_argv_mode() {
        let args = vec![
            "/Apps/WhisperPilot".to_string(),
            WORKER_FLAG.to_string(),
            "/tmp/request.json".to_string(),
        ];

        assert_eq!(
            worker_request_path(&args),
            Some(std::path::PathBuf::from("/tmp/request.json"))
        );
    }

    #[test]
    fn worker_request_path_is_absent_for_a_normal_app_launch() {
        let args = vec!["/Apps/WhisperPilot".to_string()];

        assert_eq!(worker_request_path(&args), None);
    }

    #[test]
    fn worker_request_round_trips() {
        let original = WorkerRequest {
            app_support_dir: std::path::PathBuf::from("/support"),
            samples_path: std::path::PathBuf::from("/cache/samples.f32"),
            output_path: std::path::PathBuf::from("/cache/turns.json"),
            variant: "titanet-large".to_string(),
            speaker_count: Some(2),
        };

        let json = serde_json::to_string(&original).unwrap();
        let round_tripped: WorkerRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(round_tripped, original);
    }

    // --- The child must not outlive the app that started it --------------

    #[test]
    fn the_orphan_watch_fires_as_soon_as_the_parents_pipe_closes() {
        let dir = tempfile::tempdir().unwrap();
        let closed = dir.path().join("already-closed");
        std::fs::write(&closed, b"").unwrap();

        // An empty file reads EOF immediately, standing in for the pipe the
        // parent's death closes.
        wait_for_parent_to_go(std::fs::File::open(&closed).unwrap());
    }

    #[test]
    fn the_orphan_watch_ignores_traffic_and_waits_for_the_close() {
        use std::io::Write as _;
        use std::sync::mpsc;

        let (mut parent_end, child_end) = std::os::unix::net::UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            wait_for_parent_to_go(child_end);
            let _ = tx.send(());
        });

        parent_end.write_all(b"the parent is still here\n").unwrap();
        parent_end.flush().unwrap();
        assert!(
            rx.recv_timeout(Duration::from_millis(250)).is_err(),
            "traffic from a live parent must not be mistaken for its death"
        );

        drop(parent_end);
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "closing the parent's end must end the watch"
        );
    }

    #[test]
    fn the_orphan_watch_retries_an_interrupted_read_instead_of_quitting() {
        use std::sync::atomic::AtomicUsize;

        struct Scripted(Arc<AtomicUsize>);
        impl Read for Scripted {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                match self.0.fetch_add(1, Ordering::SeqCst) {
                    0 => Err(std::io::Error::from(std::io::ErrorKind::Interrupted)),
                    1 => {
                        buf[0] = b'\n';
                        Ok(1)
                    }
                    _ => Ok(0),
                }
            }
        }

        let reads = Arc::new(AtomicUsize::new(0));
        wait_for_parent_to_go(Scripted(Arc::clone(&reads)));

        // Treating the interruption as the parent's death would stop at one
        // read — and abandon a live inference pass.
        assert_eq!(reads.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn the_child_is_given_a_pipe_on_stdin_so_it_can_notice_the_parent_dying() {
        let dir = tempfile::tempdir().unwrap();
        let output = out_path(dir.path());
        let payload = turns_json(&[(0, 100, 0)]);
        // Only produces a result when stdin really is a pipe. With
        // `Stdio::null()` the child has no handle whose close it could
        // observe, so the orphan watch could never fire.
        let mut command = shell(&format!(
            "if [ -p /dev/stdin ]; then printf '%s' '{}' > {}; fi",
            payload,
            output.display()
        ));

        let run = supervise(&mut command, &output, Duration::from_secs(10)).unwrap();

        assert!(
            matches!(run.outcome, ChildOutcome::Completed(_)),
            "the child's stdin must be a pipe, not null, got {:?}",
            run.outcome
        );
    }

    // --- Stale transports must not accumulate -----------------------------

    #[test]
    fn sweeping_removes_transport_files_older_than_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join("samples-123-0.f32");
        std::fs::write(&stale, b"leftover").unwrap();

        sweep_stale_transports(dir.path(), Duration::ZERO);

        assert!(
            !stale.exists(),
            "a transport file left behind by a killed parent must be swept"
        );
    }

    #[test]
    fn sweeping_keeps_transport_files_from_a_run_still_in_flight() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("samples-123-0.f32");
        std::fs::write(&live, b"in flight").unwrap();

        sweep_stale_transports(dir.path(), Duration::from_secs(3600));

        assert!(
            live.exists(),
            "sweeping must not delete the transport of a concurrent run"
        );
    }

    // --- Transport parsing rejects what it cannot faithfully read ---------

    #[test]
    fn reading_a_truncated_samples_file_is_an_error_not_a_silent_short_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("truncated.f32");
        // 6 bytes: one whole f32 plus half of another.
        std::fs::write(&path, [0u8, 0, 0, 0, 1, 2]).unwrap();

        let error = read_samples(&path).expect_err("a partial sample must not be silently dropped");

        assert!(matches!(error, crate::error::AppError::Diarization(_)));
    }

    #[test]
    fn a_clean_exit_with_malformed_result_json_is_a_failure_not_a_success() {
        let dir = tempfile::tempdir().unwrap();
        let output = out_path(dir.path());
        let mut command = shell(&format!("printf 'not json' > {}", output.display()));

        let run = supervise(&mut command, &output, Duration::from_secs(10)).unwrap();

        match run.outcome {
            ChildOutcome::Failed { code, .. } => assert_eq!(code, Some(0)),
            other => panic!("malformed output is not a completed run, got {other:?}"),
        }
    }

    #[test]
    fn the_worker_flag_without_a_path_is_not_a_worker_launch() {
        let args = vec!["/Apps/WhisperPilot".to_string(), WORKER_FLAG.to_string()];

        assert_eq!(worker_request_path(&args), None);
    }

    // --- A child that finishes in the timeout window is not a timeout -----

    #[test]
    fn a_child_that_had_already_exited_reports_its_real_status_not_a_timeout() {
        use std::os::unix::process::ExitStatusExt;
        let dir = tempfile::tempdir().unwrap();
        let output = out_path(dir.path());
        std::fs::write(&output, turns_json(&[(0, 100, 0)])).unwrap();

        // The kill raced a child that was already done: reaping it returned a
        // real status, so discarding that in favour of TimedOut would report
        // a successful run as a hang.
        let outcome = outcome_after_kill(
            Some(std::process::ExitStatus::from_raw(0)),
            &output,
            String::new(),
        );

        assert!(
            matches!(outcome, ChildOutcome::Completed(_)),
            "a reaped real status must win over the timeout, got {outcome:?}"
        );
    }

    #[test]
    fn the_supervisors_own_kill_reports_a_timeout_not_a_crash() {
        use std::os::unix::process::ExitStatusExt;
        let dir = tempfile::tempdir().unwrap();

        // SIGKILL is what `Child::kill` sends, so this is the hung-child path.
        assert_eq!(
            outcome_after_kill(
                Some(std::process::ExitStatus::from_raw(SIGKILL)),
                &out_path(dir.path()),
                String::new(),
            ),
            ChildOutcome::TimedOut
        );
    }

    #[test]
    fn a_real_fault_inside_the_kill_window_is_still_reported_as_a_crash() {
        use std::os::unix::process::ExitStatusExt;
        let dir = tempfile::tempdir().unwrap();

        // SIGBUS arriving in the same window as the kill is the native fault,
        // not our doing — and it is the outcome a fallback would retry on.
        assert_eq!(
            outcome_after_kill(
                Some(std::process::ExitStatus::from_raw(10)),
                &out_path(dir.path()),
                String::new(),
            ),
            ChildOutcome::Crashed { signal: 10 }
        );
    }

    #[test]
    fn a_child_that_was_genuinely_hung_reports_a_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let output = out_path(dir.path());

        assert_eq!(
            outcome_after_kill(None, &output, String::new()),
            ChildOutcome::TimedOut
        );
    }

    #[test]
    fn samples_round_trip_through_the_transport_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("samples.f32");
        let samples = vec![0.0_f32, -1.0, 0.5, f32::MIN_POSITIVE];

        write_samples(&path, &samples).unwrap();

        assert_eq!(read_samples(&path).unwrap(), samples);
    }

    // --- DoD C-2: every failure outcome stays fail-open, distinguishably --

    #[test]
    fn a_crash_becomes_a_diarization_error_naming_the_signal() {
        let error = ChildOutcome::Crashed { signal: 10 }
            .into_result()
            .expect_err("a native crash must not be reported as success");

        assert!(matches!(error, crate::error::AppError::Diarization(_)));
        assert!(
            error.to_string().contains("10"),
            "the signal must survive so a crash is distinguishable: {error}"
        );
    }

    #[test]
    fn a_timeout_becomes_a_diarization_error_distinct_from_a_crash() {
        let timed_out = ChildOutcome::TimedOut
            .into_result()
            .unwrap_err()
            .to_string();
        let crashed = ChildOutcome::Crashed { signal: 10 }
            .into_result()
            .unwrap_err()
            .to_string();

        assert_ne!(
            timed_out, crashed,
            "WP-57 retries a crash but not a timeout, so these must not share a message"
        );
    }

    #[test]
    fn an_engine_error_becomes_a_diarization_error_carrying_its_detail() {
        let error = ChildOutcome::Failed {
            code: Some(3),
            detail: "embedding model missing".to_string(),
        }
        .into_result()
        .unwrap_err();

        assert!(error.to_string().contains("embedding model missing"));
    }

    #[test]
    fn a_completed_run_returns_its_turns() {
        let turns = vec![crate::diarize::SpeakerTurn {
            start_ms: 0,
            end_ms: 900,
            speaker: 0,
        }];

        assert_eq!(
            ChildOutcome::Completed(turns.clone())
                .into_result()
                .unwrap(),
            turns
        );
    }

    // --- DoD C-1, C-5: fallback decision helpers ---------------------------

    #[test]
    fn fallback_variant_for_returns_campplus_for_titanet_large() {
        assert_eq!(
            super::fallback_variant_for("titanet-large"),
            Some("campplus")
        );
    }

    #[test]
    fn fallback_variant_for_returns_titanet_large_for_campplus() {
        assert_eq!(
            super::fallback_variant_for("campplus"),
            Some("titanet-large")
        );
    }

    #[test]
    fn fallback_variant_for_returns_none_for_unknown_variant() {
        assert_eq!(super::fallback_variant_for("unknown-model"), None);
    }

    #[test]
    fn should_retry_diarization_is_true_for_crashed() {
        assert!(super::should_retry_diarization(&ChildOutcome::Crashed {
            signal: 10
        }));
    }

    #[test]
    fn should_retry_diarization_is_false_for_timed_out() {
        assert!(!super::should_retry_diarization(&ChildOutcome::TimedOut));
    }

    #[test]
    fn should_retry_diarization_is_false_for_failed() {
        assert!(!super::should_retry_diarization(&ChildOutcome::Failed {
            code: Some(3),
            detail: "engine error".to_string(),
        }));
    }

    #[test]
    fn should_retry_diarization_is_false_for_completed() {
        let turns = vec![crate::diarize::SpeakerTurn {
            start_ms: 0,
            end_ms: 100,
            speaker: 0,
        }];
        assert!(!super::should_retry_diarization(&ChildOutcome::Completed(
            turns
        )));
    }

    #[test]
    fn variant_display_name_returns_label_for_known_variants() {
        assert_eq!(
            super::variant_display_name("titanet-large"),
            "TitaNet-large"
        );
        assert_eq!(super::variant_display_name("campplus"), "CAM++");
    }

    #[test]
    fn variant_display_name_returns_raw_id_for_unknown() {
        assert_eq!(super::variant_display_name("unknown"), "unknown");
    }
}
