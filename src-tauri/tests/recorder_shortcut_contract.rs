use whisperpilot_lib::live_capture::LiveCaptureSource;
use whisperpilot_lib::recorder_shortcut::{
    RecorderShortcut, RecorderShortcutAction, RecorderShortcutCaptureState,
    RecorderShortcutContext, RecorderShortcutEvent, RecorderShortcutGate, RecorderShortcutManager,
    ShortcutRegistry, DEFAULT_RECORDER_SHORTCUT,
};

#[derive(Default)]
struct FakeRegistry {
    calls: Vec<String>,
    reject: Option<String>,
}

impl ShortcutRegistry for FakeRegistry {
    type Error = String;

    fn register(&mut self, shortcut: &RecorderShortcut) -> Result<(), Self::Error> {
        self.calls.push(format!("register:{shortcut}"));
        if self.reject.as_deref() == Some(shortcut.as_str()) {
            Err("shortcut conflict".into())
        } else {
            Ok(())
        }
    }

    fn unregister(&mut self, shortcut: &RecorderShortcut) -> Result<(), Self::Error> {
        self.calls.push(format!("unregister:{shortcut}"));
        Ok(())
    }
}

fn context(
    state: RecorderShortcutCaptureState,
    owner: Option<LiveCaptureSource>,
) -> RecorderShortcutContext {
    RecorderShortcutContext { state, owner }
}

#[test]
fn default_and_configurable_accelerators_are_canonical_and_validated() {
    assert_eq!(DEFAULT_RECORDER_SHORTCUT, "Control+Option+Space");
    assert_eq!(
        RecorderShortcut::parse(DEFAULT_RECORDER_SHORTCUT)
            .unwrap()
            .as_str(),
        DEFAULT_RECORDER_SHORTCUT
    );
    assert_eq!(
        RecorderShortcut::parse("ctrl+alt+r").unwrap().as_str(),
        "Control+Option+R"
    );
    assert!(RecorderShortcut::parse("Space").is_err());
    assert!(RecorderShortcut::parse("Control+Option").is_err());
    assert!(RecorderShortcut::parse("Control+Option+NotAKey").is_err());
}

#[test]
fn initial_registration_failure_leaves_the_shortcut_disabled() {
    let registry = FakeRegistry {
        reject: Some(DEFAULT_RECORDER_SHORTCUT.into()),
        ..FakeRegistry::default()
    };
    let mut manager = RecorderShortcutManager::new(registry);

    assert!(manager.register_initial(DEFAULT_RECORDER_SHORTCUT).is_err());
    assert_eq!(manager.current(), None);
    assert_eq!(
        manager.registry().calls,
        vec!["register:Control+Option+Space"]
    );
}

#[test]
fn replacement_registers_new_first_and_preserves_old_on_conflict() {
    let mut manager = RecorderShortcutManager::new(FakeRegistry::default());
    manager.register_initial(DEFAULT_RECORDER_SHORTCUT).unwrap();
    manager.replace("Control+Option+R").unwrap();
    assert_eq!(manager.current().unwrap().as_str(), "Control+Option+R");
    assert_eq!(
        manager.registry().calls,
        vec![
            "register:Control+Option+Space",
            "register:Control+Option+R",
            "unregister:Control+Option+Space",
        ]
    );

    manager.registry_mut().reject = Some("Control+Option+D".into());
    assert!(manager.replace("Control+Option+D").is_err());
    assert_eq!(manager.current().unwrap().as_str(), "Control+Option+R");
    assert_eq!(
        manager.registry().calls.last().map(String::as_str),
        Some("register:Control+Option+D")
    );
    assert!(!manager
        .registry()
        .calls
        .iter()
        .any(|call| call == "unregister:Control+Option+R"));
}

#[test]
fn repeated_press_collapses_until_release() {
    let mut gate = RecorderShortcutGate::default();
    let idle = context(RecorderShortcutCaptureState::Idle, None);

    assert_eq!(
        gate.handle(RecorderShortcutEvent::Pressed { repeat: false }, idle),
        RecorderShortcutAction::StartRecorder
    );
    assert_eq!(
        gate.handle(RecorderShortcutEvent::Pressed { repeat: true }, idle),
        RecorderShortcutAction::Ignore
    );
    assert_eq!(
        gate.handle(RecorderShortcutEvent::Pressed { repeat: false }, idle),
        RecorderShortcutAction::Ignore
    );
    assert_eq!(
        gate.handle(RecorderShortcutEvent::Released, idle),
        RecorderShortcutAction::Ignore
    );
    assert_eq!(
        gate.handle(RecorderShortcutEvent::Pressed { repeat: false }, idle),
        RecorderShortcutAction::StartRecorder
    );
}

#[test]
fn shortcut_toggles_only_recorder_and_ignores_non_actionable_phases() {
    let cases = [
        (
            context(
                RecorderShortcutCaptureState::Capturing,
                Some(LiveCaptureSource::Recorder),
            ),
            RecorderShortcutAction::StopRecorder,
        ),
        (
            context(
                RecorderShortcutCaptureState::Capturing,
                Some(LiveCaptureSource::Streaming),
            ),
            RecorderShortcutAction::RejectOtherLiveSource,
        ),
        (
            context(
                RecorderShortcutCaptureState::Stopping,
                Some(LiveCaptureSource::Recorder),
            ),
            RecorderShortcutAction::Ignore,
        ),
        (
            context(
                RecorderShortcutCaptureState::Finalizing,
                Some(LiveCaptureSource::Recorder),
            ),
            RecorderShortcutAction::Ignore,
        ),
    ];

    for (context, expected) in cases {
        let mut gate = RecorderShortcutGate::default();
        assert_eq!(
            gate.handle(RecorderShortcutEvent::Pressed { repeat: false }, context,),
            expected
        );
    }
}
