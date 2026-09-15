use super::*;
use std::sync::Arc;

#[test]
fn take_window_returns_none_below_the_threshold() {
    let mut buffer = vec![0.0_f32; WINDOW_SAMPLES - 1];
    assert!(take_window(&mut buffer).is_none());
    // Nothing was drained.
    assert_eq!(buffer.len(), WINDOW_SAMPLES - 1);
}

#[test]
fn take_window_drains_exactly_one_window_and_keeps_the_remainder() {
    let mut buffer = vec![0.0_f32; WINDOW_SAMPLES + 100];
    let window = take_window(&mut buffer).expect("buffer has a full window");
    assert_eq!(window.len(), WINDOW_SAMPLES);
    assert_eq!(buffer.len(), 100);
}

#[test]
fn take_window_preserves_sample_order() {
    let mut buffer: Vec<f32> = (0..WINDOW_SAMPLES as u32 + 5).map(|i| i as f32).collect();
    let window = take_window(&mut buffer).unwrap();
    assert_eq!(window[0], 0.0);
    assert_eq!(window[WINDOW_SAMPLES - 1], (WINDOW_SAMPLES - 1) as f32);
    // Remainder starts where the window left off.
    assert_eq!(buffer[0], WINDOW_SAMPLES as f32);
}

#[test]
fn take_window_can_be_called_repeatedly_on_a_multi_window_buffer() {
    let mut buffer = vec![0.0_f32; WINDOW_SAMPLES * 2 + 1];
    assert!(take_window(&mut buffer).is_some());
    assert!(take_window(&mut buffer).is_some());
    assert!(take_window(&mut buffer).is_none());
    assert_eq!(buffer.len(), 1);
}

#[test]
fn commit_window_moves_to_the_end_of_a_contiguous_quiet_pause() {
    let mut buffer = vec![0.05_f32; WINDOW_SAMPLES + 100];
    let quiet_start = WINDOW_SAMPLES - SAMPLE_RATE as usize / 2;
    let quiet_pause_samples = VAD_FRAME_SAMPLES * 5;
    buffer[quiet_start..quiet_start + quiet_pause_samples].fill(0.0);

    let window = take_commit_window(&mut buffer).expect("commit window");

    assert_eq!(window.len(), quiet_start + quiet_pause_samples);
    assert_eq!(buffer.len(), WINDOW_SAMPLES + 100 - window.len());
}

#[test]
fn commit_window_ignores_one_quiet_frame_inside_continuous_speech() {
    let mut buffer = vec![0.05_f32; WINDOW_SAMPLES + 100];
    let transient_start = WINDOW_SAMPLES - SAMPLE_RATE as usize / 2;
    buffer[transient_start..transient_start + VAD_FRAME_SAMPLES].fill(0.0);

    let window = take_commit_window(&mut buffer).expect("commit window");

    assert_eq!(
        window.len(),
        WINDOW_SAMPLES,
        "one quiet 20 ms frame is not a speech pause and must not move the boundary"
    );
}

#[test]
fn commit_window_ends_a_natural_utterance_before_the_hard_window_limit() {
    let speech_samples = SAMPLE_RATE as usize * 3;
    let pause_samples = SAMPLE_RATE as usize * 3 / 5;
    let mut buffer = vec![0.05_f32; speech_samples];
    buffer.extend(vec![0.0_f32; pause_samples]);

    let window = take_commit_window(&mut buffer).expect("natural pause commits utterance");

    assert_eq!(window.len(), speech_samples + pause_samples);
    assert!(buffer.is_empty());
    assert!(window.len() < WINDOW_SAMPLES);
}

#[test]
fn commit_window_adapts_to_a_steady_background_noise_floor() {
    let speech_samples = SAMPLE_RATE as usize * 3;
    let pause_samples = NATURAL_PAUSE_SAMPLES;
    let mut buffer = vec![0.04_f32; speech_samples];
    buffer.extend(vec![0.006_f32; pause_samples]);

    let window = take_commit_window(&mut buffer).expect("background-relative pause commits");

    assert_eq!(window.len(), speech_samples + pause_samples);
    assert!(buffer.is_empty());
}

#[test]
fn commit_window_adapts_after_a_long_utterance() {
    let speech_samples = SAMPLE_RATE as usize * 12;
    let pause_samples = NATURAL_PAUSE_SAMPLES;
    let mut buffer = vec![0.04_f32; speech_samples];
    buffer.extend(vec![0.006_f32; pause_samples]);

    let window = take_commit_window(&mut buffer)
        .expect("a noisy pause must still commit after a long utterance");

    assert_eq!(window.len(), speech_samples + pause_samples);
    assert!(buffer.is_empty());
}

#[test]
fn steady_background_noise_without_speech_is_not_a_natural_commit() {
    let mut buffer = vec![0.006_f32; SAMPLE_RATE as usize * 4];

    assert!(take_commit_window(&mut buffer).is_none());
    assert_eq!(buffer.len(), SAMPLE_RATE as usize * 4);
}

#[test]
fn window_start_ms_is_zero_for_the_first_window() {
    assert_eq!(window_start_ms(0), 0);
}

#[test]
fn window_start_ms_advances_by_the_window_length_each_time() {
    let first = window_start_ms(0);
    let second = window_start_ms(1);
    assert_eq!(second - first, (WINDOW_SECONDS * 1000.0) as u64);
}

#[test]
fn whisper_usage_guard_grants_exclusive_access() {
    let state = AtomicU8::new(IDLE);

    let guard = WhisperUsageGuard::acquire(&state, WhisperUser::Streaming)
        .expect("idle state must grant the first acquire");

    let contended = WhisperUsageGuard::acquire(&state, WhisperUser::Meeting);
    assert_eq!(
        contended.expect_err("a held guard must block a second acquire"),
        WhisperUser::Streaming,
        "the error must name who is currently holding it"
    );

    drop(guard);

    WhisperUsageGuard::acquire(&state, WhisperUser::Meeting)
        .expect("releasing the first guard must allow a new acquire");
}

#[test]
fn whisper_usage_guard_releases_on_drop_even_after_a_contended_attempt() {
    let state = AtomicU8::new(IDLE);
    {
        let _guard = WhisperUsageGuard::acquire(&state, WhisperUser::Streaming).unwrap();
        assert!(WhisperUsageGuard::acquire(&state, WhisperUser::Streaming).is_err());
    }
    assert!(WhisperUsageGuard::acquire(&state, WhisperUser::Streaming).is_ok());
}

#[test]
fn try_claim_streaming_then_release_allows_a_later_claim() {
    let state = AtomicU8::new(IDLE);

    try_claim_streaming(&state).expect("idle state must grant the claim");
    assert_eq!(
        try_claim_streaming(&state).expect_err("a held claim must block a second one"),
        WhisperUser::Streaming
    );

    release_whisper_busy(&state);

    try_claim_streaming(&state).expect("releasing must allow a new claim");
}

#[test]
fn recorder_claim_remains_observable_until_pipeline_release() {
    let state = AtomicU8::new(IDLE);
    try_claim_recorder(&state).expect("Recorder claim");

    assert_eq!(current_whisper_user(&state), Some(WhisperUser::Recorder));

    release_whisper_busy(&state);
    assert_eq!(current_whisper_user(&state), None);
}

#[test]
fn try_claim_streaming_names_a_meeting_holder() {
    let state = AtomicU8::new(IDLE);
    let _guard = WhisperUsageGuard::acquire(&state, WhisperUser::Meeting).unwrap();

    assert_eq!(
        try_claim_streaming(&state).expect_err("a Meeting hold must block Streaming"),
        WhisperUser::Meeting
    );
}

#[test]
fn window_ms_matches_window_seconds_in_milliseconds() {
    assert_eq!(WINDOW_MS, (WINDOW_SECONDS * 1000.0) as u64);
}

#[test]
fn whisper_streaming_prompt_seeds_punctuation_and_preserves_recent_context() {
    let prompt = whisper_streaming_prompt(Some("Продолжаем обсуждение"));

    assert!(prompt.contains("Здравствуйте!"));
    assert!(prompt.contains("Hello!"));
    assert!(prompt.ends_with("Продолжаем обсуждение"));
}

#[test]
fn result_queue_keeps_committed_results_losslessly() {
    let (tx, rx) = result_channel();
    let result = |window_index| WindowResult {
        kind: WindowResultKind::Committed,
        window_index,
        start_ms: 0,
        end_ms: 1,
        decode_ms: 0,
        outcome: Ok(Transcription {
            segments: Vec::new(),
            language: "en".to_string(),
        }),
    };

    for window_index in 0..COMMITTED_RESULT_QUEUE_CAPACITY as u64 {
        tx.send(result(window_index))
            .expect("committed results must not be dropped or backpressured");
    }
    for window_index in 0..COMMITTED_RESULT_QUEUE_CAPACITY as u64 {
        assert_eq!(
            rx.recv().expect("lossless committed result").window_index,
            window_index
        );
    }
}

// Durable output has a hard memory bound. Once persistence has stopped
// draining it, the decoder must fail immediately with an observable terminal
// reason rather than allocating without limit or silently losing text.
#[test]
fn committed_result_queue_overload_is_terminal_and_preserves_accepted_results() {
    let (tx, rx) = result_channel();
    let result = |window_index| WindowResult {
        kind: WindowResultKind::Committed,
        window_index,
        start_ms: 0,
        end_ms: 1,
        decode_ms: 0,
        outcome: Ok(Transcription {
            segments: Vec::new(),
            language: "en".to_string(),
        }),
    };

    for window_index in 0..COMMITTED_RESULT_QUEUE_CAPACITY as u64 {
        tx.send(result(window_index))
            .expect("results within the fixed committed capacity are durable");
    }

    let started = Instant::now();
    let rejected = tx
        .send(result(COMMITTED_RESULT_QUEUE_CAPACITY as u64))
        .expect_err("the first result beyond the committed capacity must fail fast");
    assert!(
        started.elapsed() < Duration::from_millis(50),
        "a full committed queue must never block the decoder"
    );
    assert_eq!(
        rejected.0.window_index, COMMITTED_RESULT_QUEUE_CAPACITY as u64,
        "the caller retains the not-yet-persisted result instead of dropping it silently"
    );
    assert_eq!(
        rx.terminal_error(),
        Some(ResultQueueTerminalError::CommittedCapacityExceeded)
    );

    for window_index in 0..COMMITTED_RESULT_QUEUE_CAPACITY as u64 {
        assert_eq!(
            rx.recv()
                .expect("accepted committed result remains available")
                .window_index,
            window_index
        );
    }
    assert!(
        rx.recv().is_err(),
        "the terminal overload disconnects the decoder"
    );
}

// A provisional revision has no durable value once a newer revision exists.
// Keep only the latest one so a slow renderer cannot accumulate stale work.
#[test]
fn result_queue_conflates_partial_revisions_to_the_latest_value() {
    let (tx, rx) = result_channel();
    let partial = |window_index: u64, text: &str| WindowResult {
        kind: WindowResultKind::Partial,
        window_index,
        start_ms: 0,
        end_ms: 1,
        decode_ms: 0,
        outcome: Ok(Transcription {
            segments: vec![crate::transcribe::Segment {
                start_ms: 0,
                end_ms: 1,
                text: text.to_string(),
                speaker_id: None,
            }],
            language: "en".to_string(),
        }),
    };

    assert!(try_send_partial(&tx, partial(0, "first draft")));
    assert!(try_send_partial(&tx, partial(0, "latest draft")));

    let result = rx.recv().expect("latest partial remains available");
    let text = &result.outcome.unwrap().segments[0].text;
    assert_eq!(text, "latest draft");
    assert!(rx.try_recv().is_err(), "older partial must be replaced");
}

// Quality results and overload gaps are lossless. They also supersede any
// partial for that same unstable boundary rather than waiting behind it.
#[test]
fn committed_result_supersedes_pending_partial_without_blocking_decoder() {
    let (tx, rx) = result_channel();
    assert!(try_send_partial(
        &tx,
        WindowResult {
            kind: WindowResultKind::Partial,
            window_index: 7,
            start_ms: 0,
            end_ms: 1,
            decode_ms: 0,
            outcome: Ok(Transcription {
                segments: Vec::new(),
                language: "en".to_string(),
            }),
        },
    ));

    let started = Instant::now();
    tx.send(WindowResult {
        kind: WindowResultKind::Committed,
        window_index: 7,
        start_ms: 0,
        end_ms: 1,
        decode_ms: 0,
        outcome: Ok(Transcription {
            segments: Vec::new(),
            language: "en".to_string(),
        }),
    })
    .expect("committed result must be accepted");

    assert!(
        started.elapsed() < Duration::from_millis(50),
        "a committed result must not wait for a slow renderer"
    );
    assert_eq!(
        rx.recv().expect("committed result").kind,
        WindowResultKind::Committed
    );
    assert!(rx.try_recv().is_err(), "superseded partial must be removed");
}

// A full renderer queue may discard a provisional revision, but it must
// never stop the quality pass for audio that has become committable.
#[test]
fn full_partial_result_queue_does_not_starve_a_later_committed_window() {
    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    samples_tx
        .send(captured_chunk(0, vec![0.05_f32; PARTIAL_MIN_SAMPLES]))
        .expect("first live partial input");
    samples_tx
        .send(captured_chunk(
            PARTIAL_MIN_SAMPLES as u64,
            vec![0.05_f32; WINDOW_SAMPLES - PARTIAL_MIN_SAMPLES],
        ))
        .expect("later committed input");
    drop(samples_tx);

    let (results_tx, results_rx) = result_channel();
    results_tx
        .send(WindowResult {
            kind: WindowResultKind::Partial,
            window_index: 41,
            start_ms: 0,
            end_ms: 1,
            decode_ms: 0,
            outcome: Ok(Transcription {
                segments: Vec::new(),
                language: "en".to_string(),
            }),
        })
        .expect("saturate the decoded-result queue with an obsolete partial");

    let profiles = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&profiles);
    let handle = std::thread::spawn(move || {
        run_windowed_decode(
            move || Ok(ProfileRecordingDecoder { profiles: recorded }),
            samples_rx,
            results_tx,
            0,
        )
    });

    let deadline = Instant::now() + Duration::from_secs(2);
    let committed_decode_started_while_backlogged = loop {
        if profiles
            .lock()
            .expect("profiles mutex")
            .contains(&SessionDecodeProfile::Quality)
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    while !handle.is_finished() {
        let _ = results_rx.recv_timeout(Duration::from_millis(50));
    }
    handle
        .join()
        .expect("decode loop thread joins after cleanup");

    assert!(
        committed_decode_started_while_backlogged,
        "a full queue of stale partials must not starve a later committed window"
    );
}

// WP-82: the decode loop must create ONE decoder (one WhisperState, one
// Metal backend) per session and reuse it for every window — not one per
// window, which put a full Metal init/free cycle on every 7s window.
struct FakeDecoder {
    decoded: Vec<usize>,
}

impl SessionDecoder for FakeDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        _context: Option<&str>,
        _profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        self.decoded.push(samples.len());
        Ok(Transcription {
            segments: vec![],
            language: "en".to_string(),
        })
    }
}

fn two_windows_channel() -> (
    std::sync::mpsc::Sender<CapturedAudioChunk>,
    std::sync::mpsc::Receiver<CapturedAudioChunk>,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(captured_chunk(0, vec![0.05_f32; WINDOW_SAMPLES * 2]))
        .expect("send two windows of samples");
    (tx, rx)
}

fn captured_chunk(start_sample: u64, samples: Vec<f32>) -> CapturedAudioChunk {
    CapturedAudioChunk {
        start_sample,
        captured_end_sample: start_sample.saturating_add(samples.len() as u64),
        samples,
    }
}

#[test]
fn decode_loop_creates_one_decoder_for_a_multi_window_session() {
    let creations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (samples_tx, samples_rx) = two_windows_channel();
    let (results_tx, results_rx) = result_channel();
    drop(samples_tx);

    let counter = Arc::clone(&creations);
    run_windowed_decode(
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(FakeDecoder {
                decoded: Vec::new(),
            })
        },
        samples_rx,
        results_tx,
        0,
    );

    assert_eq!(
        creations.load(Ordering::SeqCst),
        1,
        "a session creates its decoder exactly once, however many windows it decodes"
    );
    let results: Vec<_> = results_rx.try_iter().collect();
    assert_eq!(results.len(), 2);
    assert!(
        results.iter().all(|r| r.outcome.is_ok()),
        "every window decoded through the one session decoder"
    );
    assert_eq!(results[0].window_index, 0);
    assert_eq!(results[1].window_index, 1);
}

#[test]
fn decode_loop_fail_opens_every_window_when_decoder_creation_fails() {
    let (samples_tx, samples_rx) = two_windows_channel();
    let (results_tx, results_rx) = result_channel();
    drop(samples_tx);

    run_windowed_decode(
        || -> crate::error::Result<FakeDecoder> {
            Err(crate::error::AppError::Transcribe(
                "Metal backend init failed".to_string(),
            ))
        },
        samples_rx,
        results_tx,
        0,
    );

    let results: Vec<_> = results_rx.try_iter().collect();
    assert_eq!(
        results.len(),
        2,
        "a session whose decoder cannot be created still answers every window"
    );
    for result in &results {
        assert!(
            matches!(result.outcome, Err(crate::error::AppError::Transcribe(_))),
            "creation failure must fail each window open, not end the session"
        );
    }
}

// WP-113 S-1: disconnect is the Stop boundary. A meaningful suffix must
// be decoded once even when it never reaches the regular hard cap.
#[test]
fn disconnect_flushes_meaningful_trailing_audio_exactly_once() {
    let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    let trailing_samples = SAMPLE_RATE as usize / 2;
    samples_tx
        .send(captured_chunk(0, vec![0.05_f32; trailing_samples]))
        .expect("send a meaningful trailing buffer");
    drop(samples_tx);
    let (results_tx, results_rx) = result_channel();

    let decoded = Arc::clone(&decoded_lengths);
    run_windowed_decode(
        move || {
            Ok(RecordingDecoder {
                decoded_lengths: decoded,
            })
        },
        samples_rx,
        results_tx,
        0,
    );

    let results: Vec<_> = results_rx.try_iter().collect();
    assert!(
        results.len() == 1,
        "Stop must commit one meaningful trailing decode, got {} results",
        results.len()
    );
    assert_eq!(results[0].kind, WindowResultKind::Committed);
    assert_eq!(
        results[0].end_ms, 500,
        "the trailing end timestamp must come from its 8,000 samples, not the 7s window size"
    );
    assert_eq!(
        *decoded_lengths.lock().expect("decoded-length mutex"),
        vec![trailing_samples],
        "the trailing samples must reach Whisper exactly once"
    );
}

// WP-113 failure partition: a final buffer can be non-empty without
// containing meaningful speech. Silence and sub-threshold noise must not
// invoke Whisper or create a committed result.
#[test]
fn disconnect_does_not_decode_silence_or_below_threshold_noise() {
    for samples in [
        vec![0.0_f32; SAMPLE_RATE as usize / 2],
        (0..SAMPLE_RATE as usize / 2)
            .map(|index| if index % 2 == 0 { 0.000_01 } else { -0.000_01 })
            .collect(),
    ] {
        let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (samples_tx, samples_rx) = std::sync::mpsc::channel();
        samples_tx
            .send(captured_chunk(0, samples))
            .expect("send trailing noise");
        drop(samples_tx);
        let (results_tx, results_rx) = result_channel();

        let decoded = Arc::clone(&decoded_lengths);
        run_windowed_decode(
            move || {
                Ok(RecordingDecoder {
                    decoded_lengths: decoded,
                })
            },
            samples_rx,
            results_tx,
            0,
        );

        assert!(results_rx.try_iter().next().is_none());
        assert!(
            decoded_lengths
                .lock()
                .expect("decoded-length mutex")
                .is_empty(),
            "silence/noise below the meaningful-audio predicate must be skipped"
        );
    }
}

#[test]
fn steady_room_noise_never_emits_a_partial_or_hard_commit() {
    let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    samples_tx
        .send(captured_chunk(
            0,
            vec![0.006_f32; WINDOW_SAMPLES + SAMPLE_RATE as usize],
        ))
        .expect("send steady room noise past the hard boundary");
    drop(samples_tx);
    let (results_tx, results_rx) = result_channel();

    let decoded = Arc::clone(&decoded_lengths);
    run_windowed_decode(
        move || {
            Ok(RecordingDecoder {
                decoded_lengths: decoded,
            })
        },
        samples_rx,
        results_tx,
        0,
    );

    assert!(results_rx.try_iter().next().is_none());
    assert!(
        decoded_lengths.lock().unwrap().is_empty(),
        "steady background noise must not invoke the ASR model"
    );
}

#[test]
fn varying_low_gain_speech_reaches_the_decoder() {
    let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    let samples = (0..WINDOW_SAMPLES)
        .map(|index| {
            if (index / VAD_FRAME_SAMPLES) % 2 == 0 {
                0.003_f32
            } else {
                0.006_f32
            }
        })
        .collect();
    samples_tx
        .send(captured_chunk(0, samples))
        .expect("send low-gain speech-like signal");
    drop(samples_tx);
    let (results_tx, results_rx) = result_channel();

    let decoded = Arc::clone(&decoded_lengths);
    run_windowed_decode(
        move || {
            Ok(RecordingDecoder {
                decoded_lengths: decoded,
            })
        },
        samples_rx,
        results_tx,
        0,
    );

    assert!(results_rx.try_iter().next().is_some());
    assert!(
        !decoded_lengths.lock().unwrap().is_empty(),
        "ambiguous low-gain audio must fail safe toward ASR, not silent deletion"
    );
}

#[test]
fn capture_gaps_are_explicit_and_keep_the_original_sample_timeline() {
    let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    samples_tx
        .send(captured_chunk(0, vec![0.05; SAMPLE_RATE as usize]))
        .expect("first second");
    samples_tx
        .send(captured_chunk(
            SAMPLE_RATE as u64 * 2,
            vec![0.05; SAMPLE_RATE as usize],
        ))
        .expect("third second after one-second overload gap");
    drop(samples_tx);
    let (results_tx, results_rx) = result_channel();

    let decoded = Arc::clone(&decoded_lengths);
    run_windowed_decode(
        move || {
            Ok(RecordingDecoder {
                decoded_lengths: decoded,
            })
        },
        samples_rx,
        results_tx,
        0,
    );

    let results: Vec<_> = results_rx.try_iter().collect();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].kind, WindowResultKind::Committed);
    assert_eq!((results[0].start_ms, results[0].end_ms), (0, 1_000));
    assert_eq!(results[1].kind, WindowResultKind::Gap);
    assert_eq!((results[1].start_ms, results[1].end_ms), (1_000, 2_000));
    assert!(results[1].outcome.is_err());
    assert_eq!(results[2].kind, WindowResultKind::Committed);
    assert_eq!((results[2].start_ms, results[2].end_ms), (2_000, 3_000));
}

#[test]
fn meaningful_short_prefix_before_capture_gap_is_included_in_failed_gap() {
    let decoded_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    let prefix_samples = SAMPLE_RATE as usize / 4;
    samples_tx
        .send(captured_chunk(0, vec![0.05; prefix_samples]))
        .expect("meaningful 250 ms prefix");
    samples_tx
        .send(captured_chunk(
            SAMPLE_RATE as u64,
            vec![0.05; SAMPLE_RATE as usize / 2],
        ))
        .expect("audio after the capture gap");
    drop(samples_tx);
    let (results_tx, results_rx) = result_channel();

    let decoded = Arc::clone(&decoded_lengths);
    run_windowed_decode(
        move || {
            Ok(RecordingDecoder {
                decoded_lengths: decoded,
            })
        },
        samples_rx,
        results_tx,
        0,
    );

    let results: Vec<_> = results_rx.try_iter().collect();
    let gap = results
        .iter()
        .find(|result| result.kind == WindowResultKind::Gap)
        .expect("the overloaded interval must be explicit");
    assert_eq!(
        (gap.start_ms, gap.end_ms),
        (0, 1_000),
        "a meaningful prefix too short to decode must be folded into the failed gap, not dropped"
    );
    assert!(gap.outcome.is_err());
    assert_eq!(
        *decoded_lengths.lock().expect("decoded-length mutex"),
        vec![SAMPLE_RATE as usize / 2],
        "the short prefix is represented by the failed gap rather than sent to Whisper"
    );
}

// WP-113 reconciliation contract: revisions share one unstable boundary,
// while a later durable commit removes every stale revision. A live renderer
// may have already shown a partial, but a delayed worker must not replay it.
#[test]
fn local_partials_can_revise_without_moving_committed_boundaries() {
    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    samples_tx
        .send(captured_chunk(0, vec![0.05_f32; WINDOW_SAMPLES]))
        .expect("send first committed window");
    samples_tx
        .send(captured_chunk(
            WINDOW_SAMPLES as u64,
            vec![0.05_f32; PARTIAL_MIN_SAMPLES],
        ))
        .expect("send initial partial");
    samples_tx
        .send(captured_chunk(
            (WINDOW_SAMPLES + PARTIAL_MIN_SAMPLES) as u64,
            vec![0.05_f32; PARTIAL_STEP_SAMPLES],
        ))
        .expect("send partial revision");
    samples_tx
        .send(captured_chunk(
            (WINDOW_SAMPLES + PARTIAL_MIN_SAMPLES + PARTIAL_STEP_SAMPLES) as u64,
            vec![0.05_f32; WINDOW_SAMPLES - PARTIAL_MIN_SAMPLES - PARTIAL_STEP_SAMPLES],
        ))
        .expect("complete second window");
    drop(samples_tx);
    let (results_tx, results_rx) = result_channel();

    run_windowed_decode(
        || Ok(RevisionDecoder { decode_index: 0 }),
        samples_rx,
        results_tx,
        0,
    );

    let results: Vec<_> = results_rx.try_iter().collect();
    assert_eq!(
        results.len(),
        2,
        "one committed result per durable boundary"
    );
    assert_eq!(results[0].kind, WindowResultKind::Committed);
    assert_eq!((results[0].start_ms, results[0].end_ms), (0, WINDOW_MS));
    assert_eq!(results[1].kind, WindowResultKind::Committed);
    assert_eq!(
        (results[1].start_ms, results[1].end_ms),
        (WINDOW_MS, WINDOW_MS * 2)
    );
    assert_eq!(
        (results[0].start_ms, results[0].end_ms),
        (0, WINDOW_MS),
        "later partial revisions must not mutate a committed boundary"
    );
}

#[test]
fn live_partials_use_fast_decode_and_commits_use_quality_decode() {
    let profiles = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    samples_tx
        .send(captured_chunk(0, vec![0.05_f32; PARTIAL_MIN_SAMPLES]))
        .unwrap();
    samples_tx
        .send(captured_chunk(
            PARTIAL_MIN_SAMPLES as u64,
            vec![0.05_f32; WINDOW_SAMPLES - PARTIAL_MIN_SAMPLES],
        ))
        .unwrap();
    drop(samples_tx);
    let (results_tx, _results_rx) = result_channel();
    let recorded = Arc::clone(&profiles);

    run_windowed_decode(
        move || Ok(ProfileRecordingDecoder { profiles: recorded }),
        samples_rx,
        results_tx,
        0,
    );

    assert_eq!(
        *profiles.lock().unwrap(),
        vec![
            SessionDecodeProfile::FastPartial,
            SessionDecodeProfile::Quality
        ]
    );
}

#[test]
fn confirmed_text_is_context_for_the_next_audio_window() {
    let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    samples_tx
        .send(captured_chunk(0, vec![0.05_f32; WINDOW_SAMPLES]))
        .unwrap();
    samples_tx
        .send(captured_chunk(
            WINDOW_SAMPLES as u64,
            vec![0.05_f32; FINAL_MIN_SAMPLES],
        ))
        .unwrap();
    drop(samples_tx);
    let (results_tx, _results_rx) = result_channel();
    let recorded = Arc::clone(&contexts);

    run_windowed_decode(
        move || Ok(ContextRecordingDecoder { contexts: recorded }),
        samples_rx,
        results_tx,
        0,
    );

    assert_eq!(
        *contexts.lock().unwrap(),
        vec![None, Some("confirmed phrase".to_string())]
    );
}

// WP-113 balanced decoding: the audio overlap at a forced boundary can
// make the model repeat the confirmed suffix at the start of the next
// committed result. Reconciliation removes only that boundary copy; the
// already-published prefix remains byte-for-byte stable.
#[test]
fn committed_boundary_overlap_is_removed_without_rewriting_confirmed_prefix() {
    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    samples_tx
        .send(captured_chunk(0, vec![0.05_f32; WINDOW_SAMPLES * 2]))
        .unwrap();
    let (results_tx, results_rx) = result_channel();
    drop(samples_tx);

    run_windowed_decode(
        || Ok(TimestampedOverlapDecoder { decode_index: 0 }),
        samples_rx,
        results_tx,
        0,
    );

    let results: Vec<_> = results_rx.try_iter().collect();
    assert_eq!(result_text(&results[0]), "сегодня обсуждаем план релиза");
    assert_eq!(
        result_text(&results[1]),
        "на завтра",
        "the repeated suffix/prefix comes from boundary overlap and must not be published twice"
    );
}

#[test]
fn hard_boundary_second_decode_includes_audio_from_previous_window() {
    let decoded = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (samples_tx, samples_rx) = std::sync::mpsc::channel();
    let samples = (0..WINDOW_SAMPLES * 2)
        .map(|index| 0.02_f32 + index as f32 / (WINDOW_SAMPLES * 4) as f32)
        .collect::<Vec<_>>();
    let first_non_overlap_sample = samples[WINDOW_SAMPLES];
    samples_tx.send(captured_chunk(0, samples)).unwrap();
    drop(samples_tx);
    let (results_tx, results_rx) = result_channel();
    let recorded = Arc::clone(&decoded);

    run_windowed_decode(
        move || Ok(BoundaryRecordingDecoder { decoded: recorded }),
        samples_rx,
        results_tx,
        0,
    );

    let results: Vec<_> = results_rx.try_iter().collect();
    let decoded = decoded.lock().unwrap();
    assert_eq!(results.len(), 2, "overlap must not create a third window");
    assert_eq!(decoded.len(), 2);
    assert!(
        decoded[1].0 > WINDOW_SAMPLES,
        "the second decode must include a short tail from the first logical window"
    );
    assert!(
        decoded[1].1 < first_non_overlap_sample,
        "the second decode must start before the non-overlapping hard boundary"
    );
}

// A full next utterance that happens to repeat the preceding phrase is
// real speech, not a partial suffix/prefix overlap. Do not erase it.
#[test]
fn genuine_adjacent_repeated_phrase_is_not_removed_as_boundary_overlap() {
    let (samples_tx, samples_rx) = two_windows_channel();
    let (results_tx, results_rx) = result_channel();
    drop(samples_tx);

    run_windowed_decode(
        || Ok(SequenceDecoder::new(["давайте начнем", "давайте начнем"])),
        samples_rx,
        results_tx,
        0,
    );

    let results: Vec<_> = results_rx.try_iter().collect();
    assert_eq!(result_text(&results[0]), "давайте начнем");
    assert_eq!(result_text(&results[1]), "давайте начнем");
}

#[test]
fn genuine_repeated_prefix_with_a_continuation_is_not_removed() {
    let (samples_tx, samples_rx) = two_windows_channel();
    let (results_tx, results_rx) = result_channel();
    drop(samples_tx);

    run_windowed_decode(
        || {
            Ok(SequenceDecoder::new([
                "сегодня обсуждаем план релиза",
                "план релиза переносим на завтра",
            ]))
        },
        samples_rx,
        results_tx,
        0,
    );

    let results: Vec<_> = results_rx.try_iter().collect();
    assert_eq!(result_text(&results[0]), "сегодня обсуждаем план релиза");
    assert_eq!(
        result_text(&results[1]),
        "план релиза переносим на завтра",
        "text alone cannot prove that a repeated phrase came from overlap audio"
    );
}

struct ContextRecordingDecoder {
    contexts: Arc<std::sync::Mutex<Vec<Option<String>>>>,
}

struct ProfileRecordingDecoder {
    profiles: Arc<std::sync::Mutex<Vec<SessionDecodeProfile>>>,
}

impl SessionDecoder for ProfileRecordingDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        _context: Option<&str>,
        profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        self.profiles.lock().unwrap().push(profile);
        Ok(Transcription {
            segments: vec![crate::transcribe::Segment {
                start_ms: 0,
                end_ms: samples_to_ms(samples.len()),
                text: "decoded".into(),
                speaker_id: None,
            }],
            language: "en".into(),
        })
    }
}

impl SessionDecoder for ContextRecordingDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        context: Option<&str>,
        _profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        self.contexts
            .lock()
            .unwrap()
            .push(context.map(str::to_string));
        Ok(Transcription {
            segments: vec![crate::transcribe::Segment {
                start_ms: 0,
                end_ms: samples_to_ms(samples.len()),
                text: "confirmed phrase".into(),
                speaker_id: None,
            }],
            language: "en".into(),
        })
    }
}

struct RecordingDecoder {
    decoded_lengths: Arc<std::sync::Mutex<Vec<usize>>>,
}

struct SequenceDecoder {
    texts: std::collections::VecDeque<String>,
}

struct TimestampedOverlapDecoder {
    decode_index: usize,
}

struct BoundaryRecordingDecoder {
    decoded: Arc<std::sync::Mutex<Vec<(usize, f32)>>>,
}

impl SessionDecoder for BoundaryRecordingDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        _context: Option<&str>,
        _profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        self.decoded
            .lock()
            .unwrap()
            .push((samples.len(), samples[0]));
        Ok(Transcription {
            segments: vec![],
            language: "en".to_string(),
        })
    }
}

impl SequenceDecoder {
    fn new<const N: usize>(texts: [&str; N]) -> Self {
        Self {
            texts: texts.into_iter().map(str::to_string).collect(),
        }
    }
}

impl SessionDecoder for SequenceDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        _context: Option<&str>,
        _profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        Ok(Transcription {
            segments: vec![crate::transcribe::Segment {
                start_ms: 0,
                end_ms: samples_to_ms(samples.len()),
                text: self.texts.pop_front().expect("one result per window"),
                speaker_id: None,
            }],
            language: "ru".to_string(),
        })
    }
}

impl SessionDecoder for TimestampedOverlapDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        _context: Option<&str>,
        _profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        let segments = if self.decode_index == 0 {
            vec![crate::transcribe::Segment {
                start_ms: 0,
                end_ms: samples_to_ms(samples.len()),
                text: "сегодня обсуждаем план релиза".to_string(),
                speaker_id: None,
            }]
        } else {
            vec![
                crate::transcribe::Segment {
                    start_ms: 0,
                    end_ms: 700,
                    text: "план релиза".to_string(),
                    speaker_id: None,
                },
                crate::transcribe::Segment {
                    start_ms: 700,
                    end_ms: samples_to_ms(samples.len()),
                    text: "на завтра".to_string(),
                    speaker_id: None,
                },
            ]
        };
        self.decode_index += 1;
        Ok(Transcription {
            segments,
            language: "ru".to_string(),
        })
    }

    fn has_reliable_segment_timestamps(&self) -> bool {
        true
    }
}

impl SessionDecoder for RecordingDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        _context: Option<&str>,
        _profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        self.decoded_lengths
            .lock()
            .expect("decoded-length mutex")
            .push(samples.len());
        Ok(Transcription {
            segments: vec![],
            language: "en".to_string(),
        })
    }
}

struct RevisionDecoder {
    decode_index: usize,
}

impl SessionDecoder for RevisionDecoder {
    fn decode_window(
        &mut self,
        samples: &[f32],
        _context: Option<&str>,
        _profile: SessionDecodeProfile,
    ) -> crate::error::Result<Transcription> {
        let text = [
            "first commit",
            "draft suffix",
            "revised suffix",
            "second commit",
        ][self.decode_index];
        self.decode_index += 1;
        Ok(Transcription {
            segments: vec![crate::transcribe::Segment {
                start_ms: 0,
                end_ms: samples.len() as u64 * 1_000 / SAMPLE_RATE as u64,
                text: text.to_string(),
                speaker_id: None,
            }],
            language: "en".to_string(),
        })
    }
}

fn result_text(result: &WindowResult) -> &str {
    result
        .outcome
        .as_ref()
        .expect("test decoder succeeds")
        .segments
        .first()
        .expect("test decoder emits text")
        .text
        .as_str()
}
