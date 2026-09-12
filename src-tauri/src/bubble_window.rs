//! Pure geometry and gesture rules shared by the native bubble coordinator
//! and its contract tests.

use serde::{Deserialize, Serialize};

pub const BUBBLE_SIZE: f64 = 120.0;
pub const DRAG_THRESHOLD: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LogicalPoint {
    pub x: f64,
    pub y: f64,
}

impl LogicalPoint {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalSize {
    pub width: f64,
    pub height: f64,
}

impl LogicalSize {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalRect {
    pub origin: LogicalPoint,
    pub size: LogicalSize,
}

impl LogicalRect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            origin: LogicalPoint::new(x, y),
            size: LogicalSize::new(width, height),
        }
    }

    fn contains(self, point: LogicalPoint) -> bool {
        point.x >= self.origin.x
            && point.y >= self.origin.y
            && point.x < self.origin.x + self.size.width
            && point.y < self.origin.y + self.size.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleGesture {
    Click,
    Drag,
}

pub fn classify_gesture(
    origin: LogicalPoint,
    current: LogicalPoint,
    threshold: f64,
) -> BubbleGesture {
    let dx = current.x - origin.x;
    let dy = current.y - origin.y;
    if dx.hypot(dy) > threshold {
        BubbleGesture::Drag
    } else {
        BubbleGesture::Click
    }
}

/// Clamp a top-left point so the whole target window remains visible. If the
/// saved point belongs to a connected display, that display owns the clamp;
/// otherwise the first rectangle is the deterministic primary fallback.
pub fn clamp_top_left(
    requested: LogicalPoint,
    window: LogicalSize,
    displays: &[LogicalRect],
) -> LogicalPoint {
    let Some(display) = displays
        .iter()
        .copied()
        .find(|display| display.contains(requested))
        .or_else(|| displays.first().copied())
    else {
        return requested;
    };
    let max_x = (display.origin.x + display.size.width - window.width).max(display.origin.x);
    let max_y = (display.origin.y + display.size.height - window.height).max(display.origin.y);
    LogicalPoint::new(
        requested.x.clamp(display.origin.x, max_x),
        requested.y.clamp(display.origin.y, max_y),
    )
}
