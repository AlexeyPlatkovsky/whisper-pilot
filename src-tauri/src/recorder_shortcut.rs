//! Pure Recorder shortcut policy, separated from the platform registry.

use crate::live_capture::LiveCaptureSource;
use std::fmt;

pub const DEFAULT_RECORDER_SHORTCUT: &str = "Control+Option+Space";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderShortcut(String);

impl RecorderShortcut {
    pub fn parse(value: &str) -> Result<Self, String> {
        let parts = value
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if parts.len() < 2 {
            return Err("Recorder shortcut needs a modifier and a key".into());
        }
        let mut control = false;
        let mut option = false;
        let mut shift = false;
        let mut command = false;
        for part in &parts[..parts.len() - 1] {
            match part.to_ascii_lowercase().as_str() {
                "control" | "ctrl" => control = true,
                "option" | "alt" => option = true,
                "shift" => shift = true,
                "command" | "cmd" | "meta" => command = true,
                other => return Err(format!("unsupported Recorder shortcut modifier: {other}")),
            }
        }
        if !(control || option || shift || command) {
            return Err("Recorder shortcut needs a modifier".into());
        }
        let key = canonical_key(parts[parts.len() - 1])?;
        let mut canonical = Vec::new();
        if control {
            canonical.push("Control".to_string());
        }
        if option {
            canonical.push("Option".to_string());
        }
        if shift {
            canonical.push("Shift".to_string());
        }
        if command {
            canonical.push("Command".to_string());
        }
        canonical.push(key);
        Ok(Self(canonical.join("+")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecorderShortcut {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn canonical_key(value: &str) -> Result<String, String> {
    let lower = value.to_ascii_lowercase();
    if lower == "space" {
        return Ok("Space".into());
    }
    if value.len() == 1 {
        let character = value.chars().next().unwrap();
        if character.is_ascii_alphanumeric() {
            return Ok(character.to_ascii_uppercase().to_string());
        }
    }
    if let Some(number) = lower.strip_prefix('f').and_then(|v| v.parse::<u8>().ok()) {
        if (1..=20).contains(&number) {
            return Ok(format!("F{number}"));
        }
    }
    Err(format!("unsupported Recorder shortcut key: {value}"))
}

pub trait ShortcutRegistry {
    type Error;

    fn register(&mut self, shortcut: &RecorderShortcut) -> Result<(), Self::Error>;
    fn unregister(&mut self, shortcut: &RecorderShortcut) -> Result<(), Self::Error>;
}

pub struct RecorderShortcutManager<R> {
    registry: R,
    current: Option<RecorderShortcut>,
}

impl<R: ShortcutRegistry> RecorderShortcutManager<R> {
    pub fn new(registry: R) -> Self {
        Self {
            registry,
            current: None,
        }
    }

    pub fn register_initial(&mut self, value: &str) -> Result<(), String>
    where
        R::Error: fmt::Display,
    {
        let shortcut = RecorderShortcut::parse(value)?;
        self.registry
            .register(&shortcut)
            .map_err(|error| error.to_string())?;
        self.current = Some(shortcut);
        Ok(())
    }

    pub fn replace(&mut self, value: &str) -> Result<(), String>
    where
        R::Error: fmt::Display,
    {
        let replacement = RecorderShortcut::parse(value)?;
        if self.current.as_ref() == Some(&replacement) {
            return Ok(());
        }
        self.registry
            .register(&replacement)
            .map_err(|error| error.to_string())?;
        if let Some(previous) = self.current.as_ref() {
            if let Err(error) = self.registry.unregister(previous) {
                let _ = self.registry.unregister(&replacement);
                return Err(error.to_string());
            }
        }
        self.current = Some(replacement);
        Ok(())
    }

    pub fn current(&self) -> Option<&RecorderShortcut> {
        self.current.as_ref()
    }

    pub fn registry(&self) -> &R {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut R {
        &mut self.registry
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderShortcutCaptureState {
    Idle,
    Starting,
    Capturing,
    Stopping,
    Finalizing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecorderShortcutContext {
    pub state: RecorderShortcutCaptureState,
    pub owner: Option<LiveCaptureSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderShortcutEvent {
    Pressed { repeat: bool },
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderShortcutAction {
    StartRecorder,
    StopRecorder,
    RejectOtherLiveSource,
    Ignore,
}

#[derive(Default)]
pub struct RecorderShortcutGate {
    pressed: bool,
}

impl RecorderShortcutGate {
    pub fn handle(
        &mut self,
        event: RecorderShortcutEvent,
        context: RecorderShortcutContext,
    ) -> RecorderShortcutAction {
        match event {
            RecorderShortcutEvent::Released => {
                self.pressed = false;
                RecorderShortcutAction::Ignore
            }
            RecorderShortcutEvent::Pressed { repeat } if repeat || self.pressed => {
                RecorderShortcutAction::Ignore
            }
            RecorderShortcutEvent::Pressed { .. } => {
                self.pressed = true;
                match context.state {
                    RecorderShortcutCaptureState::Idle | RecorderShortcutCaptureState::Error => {
                        RecorderShortcutAction::StartRecorder
                    }
                    RecorderShortcutCaptureState::Capturing => match context.owner {
                        Some(LiveCaptureSource::Recorder) => RecorderShortcutAction::StopRecorder,
                        Some(LiveCaptureSource::Streaming) => {
                            RecorderShortcutAction::RejectOtherLiveSource
                        }
                        None => RecorderShortcutAction::Ignore,
                    },
                    RecorderShortcutCaptureState::Starting
                    | RecorderShortcutCaptureState::Stopping
                    | RecorderShortcutCaptureState::Finalizing => RecorderShortcutAction::Ignore,
                }
            }
        }
    }
}
