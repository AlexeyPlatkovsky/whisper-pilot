//! Non-blocking decode-result delivery for the live Meeting pipeline.

use super::{WindowResult, WindowResultKind};
use std::collections::VecDeque;
use std::sync::mpsc::{RecvError, RecvTimeoutError, SendError, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Durable Meeting windows wait for the persistence worker only up to this
/// fixed bound. A full queue terminates the session explicitly: retaining an
/// unbounded transcript in memory would eventually lose process stability,
/// while dropping a committed window would make the persisted transcript lie.
pub const COMMITTED_RESULT_QUEUE_CAPACITY: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultQueueTerminalError {
    CommittedCapacityExceeded,
}

impl ResultQueueTerminalError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::CommittedCapacityExceeded => {
                "Meeting stopped because transcript persistence could not keep up with live decoding."
            }
        }
    }
}

/// A result channel gives durable work priority over display revisions.
/// Partial text is one conflated slot. Accepted committed text and capture gaps
/// remain lossless until the persistence worker receives them. At the fixed
/// durable capacity the channel fails closed with a terminal overload signal;
/// the decoder therefore never waits for a slow renderer or SQLite write.
struct ResultQueue {
    state: Mutex<ResultQueueState>,
    ready: Condvar,
}

struct ResultQueueState {
    results: VecDeque<WindowResult>,
    sender_count: usize,
    receiver_alive: bool,
    terminal_error: Option<ResultQueueTerminalError>,
}

pub struct ResultSender {
    inner: Arc<ResultQueue>,
}

pub struct ResultReceiver {
    inner: Arc<ResultQueue>,
}

pub struct ResultTryIter<'a> {
    receiver: &'a ResultReceiver,
}

impl ResultSender {
    /// Sends a committed result or gap without blocking the decoder. It also
    /// invalidates the current provisional revision for that live boundary.
    pub fn send(&self, result: WindowResult) -> std::result::Result<(), SendError<WindowResult>> {
        self.enqueue_committed(result)
    }

    pub(super) fn send_partial(
        &self,
        result: WindowResult,
    ) -> std::result::Result<(), SendError<WindowResult>> {
        self.enqueue_partial(result)
    }

    fn enqueue_committed(
        &self,
        result: WindowResult,
    ) -> std::result::Result<(), SendError<WindowResult>> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result queue mutex poisoned");
        if !state.receiver_alive {
            return Err(SendError(result));
        }
        // One revision is enough. Removing it before queuing a committed item
        // makes final results lossless even while UI delivery is delayed.
        state
            .results
            .retain(|queued| queued.kind != WindowResultKind::Partial);
        let committed_count = state
            .results
            .iter()
            .filter(|queued| queued.kind != WindowResultKind::Partial)
            .count();
        if committed_count >= COMMITTED_RESULT_QUEUE_CAPACITY {
            state.terminal_error = Some(ResultQueueTerminalError::CommittedCapacityExceeded);
            // Closing the delivery side makes every decoder sender return
            // immediately. The receiver still drains accepted windows before
            // exposing the terminal error to the persistence lifecycle.
            state.receiver_alive = false;
            self.inner.ready.notify_all();
            return Err(SendError(result));
        }
        state.results.push_back(result);
        self.inner.ready.notify_one();
        Ok(())
    }

    fn enqueue_partial(
        &self,
        result: WindowResult,
    ) -> std::result::Result<(), SendError<WindowResult>> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result queue mutex poisoned");
        if !state.receiver_alive {
            return Err(SendError(result));
        }
        state
            .results
            .retain(|queued| queued.kind != WindowResultKind::Partial);
        state.results.push_back(result);
        self.inner.ready.notify_one();
        Ok(())
    }
}

impl Clone for ResultSender {
    fn clone(&self) -> Self {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result queue mutex poisoned");
        state.sender_count = state.sender_count.saturating_add(1);
        drop(state);
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for ResultSender {
    fn drop(&mut self) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result queue mutex poisoned");
        state.sender_count = state.sender_count.saturating_sub(1);
        self.inner.ready.notify_all();
    }
}

impl ResultReceiver {
    /// Returns the terminal delivery reason after any accepted windows have
    /// been drained. Callers use it to fail the live session rather than
    /// incorrectly marking an overload-truncated transcript as complete.
    pub fn terminal_error(&self) -> Option<ResultQueueTerminalError> {
        self.inner
            .state
            .lock()
            .expect("result queue mutex poisoned")
            .terminal_error
    }

    pub fn recv(&self) -> std::result::Result<WindowResult, RecvError> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result queue mutex poisoned");
        loop {
            if let Some(result) = state.results.pop_front() {
                return Ok(result);
            }
            if state.sender_count == 0 || !state.receiver_alive {
                return Err(RecvError);
            }
            state = self
                .inner
                .ready
                .wait(state)
                .expect("result queue mutex poisoned");
        }
    }

    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<WindowResult, RecvTimeoutError> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result queue mutex poisoned");
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(result) = state.results.pop_front() {
                return Ok(result);
            }
            if state.sender_count == 0 || !state.receiver_alive {
                return Err(RecvTimeoutError::Disconnected);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(RecvTimeoutError::Timeout);
            }
            let (next, wait) = self
                .inner
                .ready
                .wait_timeout(state, remaining)
                .expect("result queue mutex poisoned");
            state = next;
            if wait.timed_out() && state.results.is_empty() {
                return if state.sender_count == 0 || !state.receiver_alive {
                    Err(RecvTimeoutError::Disconnected)
                } else {
                    Err(RecvTimeoutError::Timeout)
                };
            }
        }
    }

    pub fn try_recv(&self) -> std::result::Result<WindowResult, TryRecvError> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result queue mutex poisoned");
        if let Some(result) = state.results.pop_front() {
            Ok(result)
        } else if state.sender_count == 0 || !state.receiver_alive {
            Err(TryRecvError::Disconnected)
        } else {
            Err(TryRecvError::Empty)
        }
    }

    pub fn try_iter(&self) -> ResultTryIter<'_> {
        ResultTryIter { receiver: self }
    }
}

impl Iterator for ResultTryIter<'_> {
    type Item = WindowResult;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.try_recv().ok()
    }
}

impl Drop for ResultReceiver {
    fn drop(&mut self) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("result queue mutex poisoned");
        state.receiver_alive = false;
        self.inner.ready.notify_all();
    }
}

pub fn result_channel() -> (ResultSender, ResultReceiver) {
    let inner = Arc::new(ResultQueue {
        state: Mutex::new(ResultQueueState {
            results: VecDeque::new(),
            sender_count: 1,
            receiver_alive: true,
            terminal_error: None,
        }),
        ready: Condvar::new(),
    });
    (
        ResultSender {
            inner: Arc::clone(&inner),
        },
        ResultReceiver { inner },
    )
}
