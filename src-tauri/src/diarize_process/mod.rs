//! Process isolation for the native diarization engine — see ADR-013 and
//! docs/architecture.md's Diarization Process Isolation section.
//!
//! The child is *this same binary* re-executed with a hidden argv mode
//! (`worker::WORKER_FLAG`); each attempt is independent. Split by concern:
//! [`transport`] (file formats), [`worker`] (child behavior), [`supervise`]
//! (parent supervisor + outcome classification). macOS only
//! (`std::os::unix::process::ExitStatusExt`).

pub(crate) mod supervise;
pub(crate) mod transport;
pub(crate) mod worker;

use crate::diarize::SpeakerTurn;
use crate::error::Result;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub use supervise::{ChildOutcome, SupervisedRun};
pub use transport::WorkerRequest;
pub use worker::{worker_exit_code, worker_request_path, WORKER_FLAG};

use supervise::prepare_and_supervise;

/// How long the child may go silent before it is presumed hung and killed.
///
/// An *inactivity* budget, not a total one, so it does not scale with
/// recording length during the embedding loop. It must still absorb two
/// genuinely silent stretches: sherpa-onnx segments the whole file before its
/// first progress callback, and clusters after the last one.
pub const DEFAULT_INACTIVITY: Duration = Duration::from_secs(120);

/// How long a transport file may sit in the cache dir before it is presumed
/// abandoned. Generous enough that no in-flight run is ever swept.
const STALE_TRANSPORT_AGE: Duration = Duration::from_secs(6 * 60 * 60);

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
    supervise::sweep_stale_transports(&work_dir, STALE_TRANSPORT_AGE);

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
