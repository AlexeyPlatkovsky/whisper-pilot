//! The worker half of the diarization process isolation: how this binary
//! recognizes and executes a worker launch (hidden argv mode), reads its
//! request, runs the in-process engine, and reports progress on stdout.

use crate::diarize;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use super::transport::{read_samples, WorkerRequest};

/// Hidden argv flag that turns a normal app launch into a diarization worker.
/// Not a user-facing CLI: `prepare_and_supervise` is its only source.
pub const WORKER_FLAG: &str = "--wp-diarize-worker";

/// Exit codes the worker uses. The parent only needs "non-zero means the
/// engine reported a real error"; these exist so a log or a bug report can say
/// *which* error without parsing stderr.
pub(crate) const EXIT_BAD_REQUEST: i32 = 2;
pub(crate) const EXIT_ENGINE_ERROR: i32 = 3;
pub(crate) const EXIT_UNWRITABLE_RESULT: i32 = 4;
pub(crate) const EXIT_PARENT_GONE: i32 = 5;

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
pub(crate) fn wait_for_parent_to_go(reader: impl Read) {
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

/// One heartbeat line on stdout. Rust's stdout is line-buffered, so the
/// newline is what actually reaches the parent's pipe.
fn report_progress(done: i32, total: i32) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "progress {done} {total}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

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
    fn the_worker_flag_without_a_path_is_not_a_worker_launch() {
        let args = vec!["/Apps/WhisperPilot".to_string(), WORKER_FLAG.to_string()];

        assert_eq!(worker_request_path(&args), None);
    }

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
}
