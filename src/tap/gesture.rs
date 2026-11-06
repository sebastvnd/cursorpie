//! The gesture state machine's states.

use crate::geometry::Point;
use std::time::Instant;

/// Where a gesture came from: the trigger button of a mouse, or a hold on a trackpad (macOS, see
/// [`touch`](super::touch)). The state machine is the same for both; the source only decides the
/// few things that differ. A trackpad hold has no button, so there is no click to hand back to the
/// app when it ends early, and only a trackpad gesture ticks the trackpad's haptic motor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// The trigger button of a mouse.
    Mouse,
    /// A hold with the fingers on a trackpad.
    Touch,
}

/// What the trigger button is doing right now. Shared (behind a mutex) between the tap callback
/// and the short-lived timer threads; `id` ties a timer to the press that started it.
pub(crate) enum Gesture {
    Idle,
    /// Pressed, nothing decided yet. `profile` = the pie opened on hold, `app` = the app's own
    /// profile (opened by a double-click), if it has one.
    Pending {
        id: u64,
        pos: Point,
        profile: usize,
        app: Option<usize>,
        source: Source,
    },
    /// First click released; waiting for a second press.
    WaitSecond {
        id: u64,
        pos: Point,
        app: usize,
        at: Instant,
    },
    /// The pie is open and the button is still down.
    ActivePie {
        id: u64,
        center: Point,
        profile: usize,
        path: Vec<usize>,
        hover: Option<usize>,
        source: Source,
    },
    /// Settings was opened by holding; swallow the rest of the gesture.
    InSettings(Source),
    /// The press was handed back to the app (a real drag); pass everything through until release.
    Passthrough,
}

impl Gesture {
    /// Who started the gesture that is running. `None` when nothing is running (`Idle`) or when the
    /// press was handed back to the app (`Passthrough`, always a mouse).
    pub(crate) fn source(&self) -> Option<Source> {
        match self {
            Gesture::Idle | Gesture::Passthrough => None,
            // a click waiting for its twin is always the mouse's: a trackpad tap measures the gap
            // itself (see `touch::recognizer`) and never waits
            Gesture::WaitSecond { .. } => Some(Source::Mouse),
            Gesture::Pending { source, .. } | Gesture::ActivePie { source, .. } => Some(*source),
            Gesture::InSettings(source) => Some(*source),
        }
    }
}
