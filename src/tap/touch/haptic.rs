//! Haptic ticks for a trackpad hold (macOS): which moments tick, and how often.
//!
//! The decision is **pure** ([`Limiter`] takes the time as a parameter), so it is unit tested on
//! every OS. The tick itself is played by the UI thread (`platform::haptic`, see
//! [`Msg::Haptic`](crate::msg::Msg::Haptic)): AppKit is never called from the tap or the touch thread.
//!
//! | Moment | Pattern | Why |
//! |---|---|---|
//! | the pie opens after the hold | [`HapticKind::LevelChange`] | a firm "it is open" confirmation |
//! | Settings opens by holding | `LevelChange` | the same, for the longer hold |
//! | the hovered slice changes | [`HapticKind::Alignment`] | a soft detent, like the optional hover sound |
//! | the extra click switches the mode | [`HapticKind::Generic`] | a distinct "click" |
//! | a slice runs, the hold is cancelled | none | the finger is lifting; silence means "nothing happened" |
//!
//! Rules: at most one tick per [`MIN_GAP`]; no tick for a hover change that only flickers back to
//! the slice it just left within [`FLICKER`], so a finger on the border of two slices does not buzz;
//! only a trackpad gesture ticks (the callers are only reached with `Source::Touch`); the settings
//! `haptics` and `haptic_hover` switch the whole thing, or the hover ticks, off. The system's own
//! "Force Click and haptic feedback" setting stays in charge: when it is off the call does nothing.

use crate::config::cfg;
use crate::msg::{Msg, Notifier};
use crate::platform::HapticKind;
use std::sync::Mutex;
use std::time::Duration;

/// At most one tick per this long.
pub const MIN_GAP: Duration = Duration::from_millis(40);
/// A hover change back to the previous slice within this long is a flicker, not a move.
pub const FLICKER: Duration = Duration::from_millis(80);

/// What happened that may tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// The pie (or Settings) opened.
    Open,
    /// The hovered slice is now this one (`None`: no slice, the center).
    Hover(Option<usize>),
    /// The extra click switched the mode.
    Mode,
}

/// Remembers the last tick and the last hover changes.
#[derive(Debug, Default)]
pub struct Limiter {
    last_tick: Option<Duration>,
    hover: Option<usize>,
    before: Option<usize>,
    changed_at: Option<Duration>,
}

impl Limiter {
    pub const fn new() -> Self {
        Self {
            last_tick: None,
            hover: None,
            before: None,
            changed_at: None,
        }
    }

    /// The pattern to play for `tick` at `now`, or `None` for silence. Every call must be reported,
    /// also the ones that stay silent, because the hover changes are remembered.
    pub fn decide(&mut self, now: Duration, tick: Tick, hover_ticks: bool) -> Option<HapticKind> {
        let kind = match tick {
            Tick::Open => {
                // a new pie: forget where the finger was
                self.hover = None;
                self.before = None;
                self.changed_at = None;
                Some(HapticKind::LevelChange)
            }
            Tick::Mode => Some(HapticKind::Generic),
            Tick::Hover(slice) => {
                let flicker = slice == self.before
                    && self
                        .changed_at
                        .is_some_and(|at| now >= at && now - at < FLICKER);
                if slice != self.hover {
                    self.before = self.hover;
                    self.hover = slice;
                    self.changed_at = Some(now);
                }
                (slice.is_some() && !flicker && hover_ticks).then_some(HapticKind::Alignment)
            }
        }?;
        let too_soon = self
            .last_tick
            .is_some_and(|t| now >= t && now - t < MIN_GAP);
        if too_soon {
            return None;
        }
        self.last_tick = Some(now);
        Some(kind)
    }
}

static LIMITER: Mutex<Limiter> = Mutex::new(Limiter::new());

/// Asks the UI thread for the tick that `tick` deserves, if the settings and the rate limit allow it.
/// Never blocks: it takes two short locks and sends a message.
pub(crate) fn emit(notifier: &Notifier, tick: Tick) {
    let (on, hover) = {
        let c = cfg();
        (c.haptics, c.haptic_hover)
    };
    if !on {
        return;
    }
    let now = super::now();
    let kind = crate::config::lock(&LIMITER).decide(now, tick, hover);
    diag::log!(
        "touch",
        "haptic",
        tick = match tick {
            Tick::Open => "open",
            Tick::Hover(_) => "hover",
            Tick::Mode => "mode",
        },
        played = kind.is_some()
    );
    if let Some(kind) = kind {
        notifier.send(Msg::Haptic(kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn opening_ticks_firmly_and_a_mode_switch_clicks() {
        let mut l = Limiter::new();
        assert_eq!(
            l.decide(MS(0), Tick::Open, true),
            Some(HapticKind::LevelChange)
        );
        assert_eq!(
            l.decide(MS(500), Tick::Mode, true),
            Some(HapticKind::Generic)
        );
    }

    #[test]
    fn a_new_slice_ticks_softly_and_the_center_is_silent() {
        let mut l = Limiter::new();
        l.decide(MS(0), Tick::Open, true);
        assert_eq!(
            l.decide(MS(200), Tick::Hover(Some(2)), true),
            Some(HapticKind::Alignment)
        );
        assert_eq!(l.decide(MS(400), Tick::Hover(None), true), None);
        assert_eq!(
            l.decide(MS(600), Tick::Hover(Some(3)), true),
            Some(HapticKind::Alignment)
        );
    }

    #[test]
    fn hover_ticks_can_be_switched_off_without_losing_the_others() {
        let mut l = Limiter::new();
        assert_eq!(l.decide(MS(0), Tick::Hover(Some(1)), false), None);
        assert_eq!(
            l.decide(MS(100), Tick::Open, false),
            Some(HapticKind::LevelChange)
        );
        assert_eq!(
            l.decide(MS(300), Tick::Mode, false),
            Some(HapticKind::Generic)
        );
    }

    #[test]
    fn nothing_ticks_twice_within_the_minimum_gap() {
        let mut l = Limiter::new();
        assert!(l.decide(MS(1000), Tick::Open, true).is_some());
        assert_eq!(l.decide(MS(1020), Tick::Hover(Some(0)), true), None);
        assert!(l.decide(MS(1100), Tick::Hover(Some(1)), true).is_some());
        assert_eq!(l.decide(MS(1139), Tick::Mode, true), None);
        assert!(l.decide(MS(1140), Tick::Mode, true).is_some());
    }

    #[test]
    fn a_finger_on_the_border_of_two_slices_does_not_buzz() {
        let mut l = Limiter::new();
        l.decide(MS(0), Tick::Open, true);
        assert!(l.decide(MS(200), Tick::Hover(Some(1)), true).is_some());
        // B -> A quickly: back to the slice just left
        assert!(l.decide(MS(300), Tick::Hover(Some(2)), true).is_some());
        assert_eq!(
            l.decide(MS(340), Tick::Hover(Some(1)), true),
            None,
            "flicker back"
        );
        assert_eq!(
            l.decide(MS(380), Tick::Hover(Some(2)), true),
            None,
            "and forth"
        );
        // a real, slower move ticks again
        assert!(l.decide(MS(600), Tick::Hover(Some(1)), true).is_some());
    }

    #[test]
    fn a_slow_return_to_the_previous_slice_is_a_real_move() {
        let mut l = Limiter::new();
        assert!(l.decide(MS(100), Tick::Hover(Some(1)), true).is_some());
        assert!(l.decide(MS(200), Tick::Hover(Some(2)), true).is_some());
        assert!(l.decide(MS(400), Tick::Hover(Some(1)), true).is_some());
    }

    #[test]
    fn opening_forgets_the_previous_pie() {
        let mut l = Limiter::new();
        l.decide(MS(0), Tick::Hover(Some(1)), true);
        l.decide(MS(100), Tick::Hover(Some(2)), true);
        l.decide(MS(1000), Tick::Open, true);
        // the same slice as before the new pie opened ticks at once: it is not a flicker
        assert!(l.decide(MS(1100), Tick::Hover(Some(1)), true).is_some());
    }

    #[test]
    fn a_clock_that_goes_backwards_never_blocks_forever() {
        let mut l = Limiter::new();
        assert!(l.decide(MS(5000), Tick::Open, true).is_some());
        assert!(l.decide(MS(10), Tick::Mode, true).is_some());
    }
}
