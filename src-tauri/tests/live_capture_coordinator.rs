use whisperpilot_lib::live_capture::{
    LiveCaptureCoordinator, LiveCapturePhase, LiveCaptureRuntimeCoordinator, LiveCaptureSource,
};

// WP-112: the backend state machine, rather than a mounted renderer, owns the
// authoritative lifecycle of the one live-capture resource.
#[test]
fn live_capture_lifecycle_has_typed_monotonic_snapshots() {
    let mut coordinator = LiveCaptureCoordinator::default();

    let idle = coordinator.snapshot();
    assert_eq!(idle.phase, LiveCapturePhase::Idle);
    assert_eq!(idle.session_id, None);
    assert_eq!(idle.generation, 0);
    assert_eq!(idle.revision, 0);

    let starting = coordinator.begin_start(41).expect("begin first capture");
    assert_eq!(starting.phase, LiveCapturePhase::Starting);
    assert_eq!(starting.session_id, Some(41));
    assert_eq!(starting.generation, 1);
    assert!(starting.revision > idle.revision);

    let capturing = coordinator
        .mark_capturing(starting.generation)
        .expect("capture started");
    assert_eq!(capturing.phase, LiveCapturePhase::Capturing);
    assert_eq!(capturing.session_id, Some(41));
    assert_eq!(capturing.generation, starting.generation);
    assert!(capturing.revision > starting.revision);

    let stopping = coordinator.begin_stop().expect("begin stop");
    assert_eq!(stopping.phase, LiveCapturePhase::Stopping);
    assert_eq!(stopping.session_id, Some(41));
    assert_eq!(stopping.generation, capturing.generation);
    assert!(stopping.revision > capturing.revision);

    let stopped = coordinator
        .finish_stop(stopping.generation)
        .expect("finish stop");
    assert_eq!(stopped.phase, LiveCapturePhase::Idle);
    assert_eq!(stopped.session_id, None);
    assert_eq!(stopped.generation, stopping.generation);
    assert!(stopped.revision > stopping.revision);
}

#[test]
fn duplicate_start_is_rejected_without_mutating_the_active_generation() {
    let mut coordinator = LiveCaptureCoordinator::default();
    let starting = coordinator.begin_start(7).expect("begin capture");

    assert!(coordinator.begin_start(8).is_err());
    assert_eq!(coordinator.snapshot(), starting);

    let capturing = coordinator
        .mark_capturing(starting.generation)
        .expect("capture started");
    assert!(coordinator.begin_start(8).is_err());
    assert_eq!(coordinator.snapshot(), capturing);
}

#[test]
fn duplicate_stop_is_idempotent_and_does_not_advance_revision_twice() {
    let mut coordinator = LiveCaptureCoordinator::default();
    let starting = coordinator.begin_start(7).expect("begin capture");
    coordinator
        .mark_capturing(starting.generation)
        .expect("capture started");

    let stopping = coordinator.begin_stop().expect("begin stop");
    let duplicate = coordinator.begin_stop().expect("duplicate stop");
    assert_eq!(duplicate, stopping);

    let idle = coordinator
        .finish_stop(stopping.generation)
        .expect("finish stop");
    let duplicate_after_finish = coordinator.begin_stop().expect("stop idle");
    assert_eq!(duplicate_after_finish, idle);
}

#[test]
fn stale_generation_callbacks_cannot_mutate_a_newer_session() {
    let mut coordinator = LiveCaptureCoordinator::default();
    let first = coordinator.begin_start(10).expect("begin first capture");
    coordinator
        .mark_capturing(first.generation)
        .expect("first capture started");
    coordinator.begin_stop().expect("begin first stop");
    coordinator
        .finish_stop(first.generation)
        .expect("finish first stop");

    let second = coordinator.begin_start(11).expect("begin second capture");
    let before_stale_callbacks = coordinator.snapshot();
    assert!(second.generation > first.generation);

    assert!(coordinator.mark_capturing(first.generation).is_err());
    assert_eq!(coordinator.snapshot(), before_stale_callbacks);

    assert!(coordinator.finish_stop(first.generation).is_err());
    assert_eq!(coordinator.snapshot(), before_stale_callbacks);

    assert!(coordinator
        .fail(first.generation, "late failure from old capture")
        .is_err());
    assert_eq!(coordinator.snapshot(), before_stale_callbacks);

    let capturing = coordinator
        .mark_capturing(second.generation)
        .expect("current capture started");
    assert_eq!(capturing.phase, LiveCapturePhase::Capturing);
    assert_eq!(capturing.session_id, Some(11));
}

#[test]
fn current_generation_failure_is_exposed_as_a_typed_error_snapshot() {
    let mut coordinator = LiveCaptureCoordinator::default();
    let starting = coordinator.begin_start(25).expect("begin capture");

    let failed = coordinator
        .fail(starting.generation, "capture permission denied")
        .expect("record current failure");

    assert_eq!(failed.phase, LiveCapturePhase::Error);
    assert_eq!(failed.session_id, Some(25));
    assert_eq!(failed.generation, starting.generation);
    assert_eq!(failed.error.as_deref(), Some("capture permission denied"));
    assert!(failed.revision > starting.revision);
}

#[test]
fn stop_during_start_rejects_the_late_runtime_and_can_finish_idle() {
    let mut coordinator = LiveCaptureRuntimeCoordinator::<String>::default();
    let starting = coordinator
        .begin_start(31, LiveCaptureSource::Streaming)
        .expect("begin capture");

    let (stopping, runtime) = coordinator.begin_stop().expect("cancel start");
    assert_eq!(stopping.phase, LiveCapturePhase::Stopping);
    assert!(runtime.is_none());

    let rejected = coordinator
        .install_runtime(starting.generation, "late native handle".to_string())
        .expect_err("a runtime arriving after Stop must not be installed");
    assert_eq!(rejected, "late native handle");

    let idle = coordinator
        .finish_stop(starting.generation)
        .expect("finish cancelled start");
    assert_eq!(idle.phase, LiveCapturePhase::Idle);
    assert_eq!(idle.session_id, None);
}

#[test]
fn stale_finish_cannot_drop_a_newer_generation_runtime() {
    let mut coordinator = LiveCaptureRuntimeCoordinator::<String>::default();
    let first = coordinator
        .begin_start(41, LiveCaptureSource::Streaming)
        .expect("begin first capture");
    coordinator
        .install_runtime(first.generation, "first runtime".to_string())
        .expect("install first runtime");
    let (_, first_runtime) = coordinator.begin_stop().expect("stop first capture");
    drop(first_runtime);
    coordinator
        .finish_stop(first.generation)
        .expect("finish first capture");

    let second = coordinator
        .begin_start(42, LiveCaptureSource::Streaming)
        .expect("begin second capture");
    coordinator
        .install_runtime(second.generation, "second runtime".to_string())
        .expect("install second runtime");

    assert!(coordinator.finish_stop(first.generation).is_err());
    let (_, runtime) = coordinator.begin_stop().expect("stop second capture");
    assert_eq!(runtime.as_deref(), Some("second runtime"));
}

#[test]
fn source_qualified_stop_preserves_a_runtime_owned_by_the_other_source() {
    for (owner, other) in [
        (LiveCaptureSource::Streaming, LiveCaptureSource::Recorder),
        (LiveCaptureSource::Recorder, LiveCaptureSource::Streaming),
    ] {
        let mut coordinator = LiveCaptureRuntimeCoordinator::<String>::default();
        let starting = coordinator
            .begin_start(51, owner)
            .expect("begin owned capture");
        let capturing = coordinator
            .install_runtime(starting.generation, format!("{owner:?} runtime"))
            .expect("install owned runtime");

        assert!(coordinator.begin_stop_for(other).is_err());
        assert_eq!(coordinator.snapshot(), capturing);

        let (stopping, runtime) = coordinator
            .begin_stop_for(owner)
            .expect("the owning source can stop capture");
        assert_eq!(stopping.phase, LiveCapturePhase::Stopping);
        assert_eq!(stopping.source, Some(owner));
        assert_eq!(
            runtime.as_deref(),
            Some(format!("{owner:?} runtime").as_str())
        );
    }
}
