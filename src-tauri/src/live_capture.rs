//! Backend-owned lifecycle for the application's single live audio capture.
//!
//! The state machine is intentionally independent of Tauri and the concrete
//! capture engine. [`LiveCaptureCoordinator`] can therefore be tested without
//! opening a microphone or ScreenCaptureKit stream, while still owning the
//! native runtime in production through its generic runtime slot.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCapturePhase {
    Idle,
    Starting,
    Capturing,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCaptureSource {
    Streaming,
    Recorder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveCaptureSnapshot {
    pub phase: LiveCapturePhase,
    pub session_id: Option<i64>,
    pub source: Option<LiveCaptureSource>,
    pub generation: u64,
    pub revision: u64,
    pub error: Option<String>,
}

impl Default for LiveCaptureSnapshot {
    fn default() -> Self {
        Self {
            phase: LiveCapturePhase::Idle,
            session_id: None,
            source: None,
            generation: 0,
            revision: 0,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveCaptureTransitionError(String);

impl fmt::Display for LiveCaptureTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LiveCaptureTransitionError {}

/// Owns the authoritative lifecycle independently of a renderer.
#[derive(Default)]
pub struct LiveCaptureCoordinator {
    snapshot: LiveCaptureSnapshot,
}

impl LiveCaptureCoordinator {
    pub fn snapshot(&self) -> LiveCaptureSnapshot {
        self.snapshot.clone()
    }

    pub fn begin_start(
        &mut self,
        session_id: i64,
    ) -> Result<LiveCaptureSnapshot, LiveCaptureTransitionError> {
        self.begin_start_with_source(session_id, LiveCaptureSource::Streaming)
    }

    pub fn begin_start_with_source(
        &mut self,
        session_id: i64,
        source: LiveCaptureSource,
    ) -> Result<LiveCaptureSnapshot, LiveCaptureTransitionError> {
        if !matches!(
            self.snapshot.phase,
            LiveCapturePhase::Idle | LiveCapturePhase::Error
        ) {
            return Err(LiveCaptureTransitionError(
                "a live capture session is already active".to_string(),
            ));
        }
        self.snapshot.generation = self.snapshot.generation.saturating_add(1);
        self.snapshot.revision = self.snapshot.revision.saturating_add(1);
        self.snapshot.phase = LiveCapturePhase::Starting;
        self.snapshot.session_id = Some(session_id);
        self.snapshot.source = Some(source);
        self.snapshot.error = None;
        Ok(self.snapshot())
    }

    pub fn mark_capturing(
        &mut self,
        generation: u64,
    ) -> Result<LiveCaptureSnapshot, LiveCaptureTransitionError> {
        self.require_generation(generation)?;
        if self.snapshot.phase != LiveCapturePhase::Starting {
            return Err(LiveCaptureTransitionError(
                "live capture is no longer starting".to_string(),
            ));
        }
        self.transition(LiveCapturePhase::Capturing, None);
        Ok(self.snapshot())
    }

    pub fn begin_stop(&mut self) -> Result<LiveCaptureSnapshot, LiveCaptureTransitionError> {
        match self.snapshot.phase {
            LiveCapturePhase::Idle | LiveCapturePhase::Stopping => Ok(self.snapshot()),
            LiveCapturePhase::Error => {
                self.transition_to_idle();
                Ok(self.snapshot())
            }
            LiveCapturePhase::Starting | LiveCapturePhase::Capturing => {
                self.transition(LiveCapturePhase::Stopping, None);
                Ok(self.snapshot())
            }
        }
    }

    pub fn finish_stop(
        &mut self,
        generation: u64,
    ) -> Result<LiveCaptureSnapshot, LiveCaptureTransitionError> {
        self.require_generation(generation)?;
        if self.snapshot.phase == LiveCapturePhase::Error {
            return Ok(self.snapshot());
        }
        if self.snapshot.phase != LiveCapturePhase::Idle {
            self.transition_to_idle();
        }
        Ok(self.snapshot())
    }

    pub fn fail(
        &mut self,
        generation: u64,
        message: impl Into<String>,
    ) -> Result<LiveCaptureSnapshot, LiveCaptureTransitionError> {
        self.require_generation(generation)?;
        if self.snapshot.phase == LiveCapturePhase::Idle {
            return Err(LiveCaptureTransitionError(
                "an idle capture cannot fail".to_string(),
            ));
        }
        self.transition(LiveCapturePhase::Error, Some(message.into()));
        Ok(self.snapshot())
    }

    fn require_generation(&self, generation: u64) -> Result<(), LiveCaptureTransitionError> {
        if self.snapshot.generation == generation {
            Ok(())
        } else {
            Err(LiveCaptureTransitionError(format!(
                "stale live capture generation {generation}; current generation is {}",
                self.snapshot.generation
            )))
        }
    }

    fn transition(&mut self, phase: LiveCapturePhase, error: Option<String>) {
        self.snapshot.phase = phase;
        self.snapshot.error = error;
        self.snapshot.revision = self.snapshot.revision.saturating_add(1);
    }

    fn transition_to_idle(&mut self) {
        self.snapshot.phase = LiveCapturePhase::Idle;
        self.snapshot.session_id = None;
        self.snapshot.source = None;
        self.snapshot.error = None;
        self.snapshot.revision = self.snapshot.revision.saturating_add(1);
    }
}

/// Production wrapper that changes lifecycle and native-runtime ownership
/// under one lock. A renderer can disappear without dropping either.
pub struct LiveCaptureRuntimeCoordinator<R> {
    lifecycle: LiveCaptureCoordinator,
    runtime: Option<R>,
}

impl<R> Default for LiveCaptureRuntimeCoordinator<R> {
    fn default() -> Self {
        Self {
            lifecycle: LiveCaptureCoordinator::default(),
            runtime: None,
        }
    }
}

impl<R> LiveCaptureRuntimeCoordinator<R> {
    pub fn snapshot(&self) -> LiveCaptureSnapshot {
        self.lifecycle.snapshot()
    }

    pub fn begin_start(
        &mut self,
        session_id: i64,
        source: LiveCaptureSource,
    ) -> Result<LiveCaptureSnapshot, LiveCaptureTransitionError> {
        self.lifecycle.begin_start_with_source(session_id, source)
    }

    /// A stale/cancelled start gets its runtime back so the caller can drop it.
    pub fn install_runtime(
        &mut self,
        generation: u64,
        runtime: R,
    ) -> Result<LiveCaptureSnapshot, R> {
        match self.lifecycle.mark_capturing(generation) {
            Ok(snapshot) => {
                self.runtime = Some(runtime);
                Ok(snapshot)
            }
            Err(_) => Err(runtime),
        }
    }

    pub fn begin_stop(
        &mut self,
    ) -> Result<(LiveCaptureSnapshot, Option<R>), LiveCaptureTransitionError> {
        let snapshot = self.lifecycle.begin_stop()?;
        let runtime = self.runtime.take();
        Ok((snapshot, runtime))
    }

    /// Stops only the requested live source. The source check and runtime
    /// removal happen under the coordinator's caller-held lock, so a delayed
    /// Streaming command cannot tear down a newer Recorder runtime (or the
    /// inverse).
    pub fn begin_stop_for(
        &mut self,
        source: LiveCaptureSource,
    ) -> Result<(LiveCaptureSnapshot, Option<R>), LiveCaptureTransitionError> {
        if let Some(owner) = self.lifecycle.snapshot.source {
            if owner != source {
                return Err(LiveCaptureTransitionError(format!(
                    "cannot stop {source:?}; the active live source is {owner:?}"
                )));
            }
        }
        self.begin_stop()
    }

    pub fn finish_stop(
        &mut self,
        generation: u64,
    ) -> Result<LiveCaptureSnapshot, LiveCaptureTransitionError> {
        let snapshot = self.lifecycle.finish_stop(generation)?;
        self.runtime = None;
        Ok(snapshot)
    }

    pub fn fail(
        &mut self,
        generation: u64,
        message: impl Into<String>,
    ) -> Result<(LiveCaptureSnapshot, Option<R>), LiveCaptureTransitionError> {
        let snapshot = self.lifecycle.fail(generation, message)?;
        Ok((snapshot, self.runtime.take()))
    }
}
