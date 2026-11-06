//! The scroll guard: what the pie swallows of the scrolling a trackpad makes, and for how long.
//!
//! While a trackpad pie is open, the finger that holds and the one that clicks are two fingers, and
//! the system scrolls with two fingers. That scroll must never reach the app under the pie.
//!
//! **The old rule** was "swallow every wheel event while a hold runs" ([`ACTIVE`]). It was right
//! for the events in the middle of the gesture and wrong at its end:
//!
//! - The gesture ends the moment a finger lifts, but the scrolling the fingers started does not:
//!   the system still delivers the last deltas, and on macOS and on Windows precision touchpads
//!   the *momentum* (the glide after the fingers left). Those events arrived after `ACTIVE` went
//!   false, so they **leaked** into the app under the pie, which scrolled by itself right after the
//!   pie closed.
//! - Dropping the first half of a scroll and passing the second half is worse than dropping or
//!   passing all of it: an app that sees the middle or the end of a scroll it never saw begin
//!   jumps, stutters or glides on its own. That is what made the pie (and the app after it) feel
//!   not smooth.
//!
//! **The rule now** is "a scroll that the pie began to swallow is swallowed to its end":
//!
//! 1. While a hold runs, every wheel event is swallowed (as before).
//! 2. When a hold that **opened** ends, a *tail* starts: wheel events keep being swallowed for
//!    [`TAIL_MS`], and each swallowed event extends the tail by [`TAIL_GAP_MS`] (so a glide is
//!    eaten to its last event), but never past [`TAIL_MAX_MS`] from the end of the gesture.
//! 3. The tail stops at once when a **new finger lands** (more fingers than at the end of the
//!    gesture): that is the person starting a scroll of their own, and it must work.
//! 4. A hold that never opened (a tap, a slide, a second finger that arrived early) starts no tail:
//!    the person was scrolling, not using the pie.
//!
//! The same rule serves the three systems. macOS and Windows ask [`swallows`] for every wheel
//! event; on Linux the wheel is grabbed as X buttons 4 to 7 for as long as [`wanted`] says so, and
//! the presses that arrive keep the tail alive through [`swallows`] as well.
//!
//! **Cost.** With no trackpad gesture and no tail, [`swallows`] is two atomic loads, asked for every
//! wheel event. Nothing is allocated and no clock is read.
//!
//! **Privacy.** Only counts and decisions are logged (`scroll_tail`, `scroll_tail_end`), never a
//! delta or a position.

use super::{ACTIVE, now};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// A tail lasts at least this long (ms) after a pie that opened closes.
pub const TAIL_MS: u64 = 300;
/// Each swallowed event of a tail keeps it alive for this long (ms) from that event.
pub const TAIL_GAP_MS: u64 = 120;
/// A tail never lasts longer than this (ms) from the end of the gesture, whatever arrives.
pub const TAIL_MAX_MS: u64 = 1200;

/// Until this time (ms on the touch clock, plus 1; 0 = no tail) wheel events are swallowed.
static TAIL_UNTIL: AtomicU64 = AtomicU64::new(0);
/// The tail ends here whatever happens (ms on the touch clock, plus 1).
static TAIL_CAP: AtomicU64 = AtomicU64::new(0);
/// Fingers on the surface at the end of the gesture, lowered as they lift: a higher count is a new touch.
static TAIL_FLOOR: AtomicUsize = AtomicUsize::new(0);
/// Wheel events swallowed in the gesture and its tail so far (for the log).
static DROPPED: AtomicU32 = AtomicU32::new(0);
/// Of those, the ones that arrived after the gesture had ended.
static DROPPED_IN_TAIL: AtomicU32 = AtomicU32::new(0);

fn now_ms() -> u64 {
    now().as_millis() as u64 + 1
}

/// Why a tail ended (for the log).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// No wheel event came for [`TAIL_GAP_MS`] after the last one (or [`TAIL_MS`] after the end).
    Quiet,
    /// It reached [`TAIL_MAX_MS`].
    Capped,
    /// A new finger landed: the person scrolls on purpose.
    NewTouch,
    /// The next trackpad gesture began.
    NextGesture,
    /// The touch source stopped.
    Stopped,
}

impl Why {
    /// The name used in the diagnostic log.
    pub fn name(self) -> &'static str {
        match self {
            Why::Quiet => "quiet",
            Why::Capped => "capped",
            Why::NewTouch => "new_touch",
            Why::NextGesture => "next_gesture",
            Why::Stopped => "stopped",
        }
    }
}

/// Whether the wheel event that is arriving now is swallowed. Called by the native taps for every
/// wheel event, so the common case (nothing running) is two atomic loads.
pub(crate) fn swallows() -> bool {
    let active = ACTIVE.load(Ordering::Relaxed);
    if !active && TAIL_UNTIL.load(Ordering::Relaxed) == 0 {
        return false;
    }
    swallows_at(now_ms())
}

fn swallows_at(t: u64) -> bool {
    if ACTIVE.load(Ordering::SeqCst) {
        DROPPED.fetch_add(1, Ordering::Relaxed);
        diag::trace!("touch", "scroll_drop", tail = false);
        return true;
    }
    let until = TAIL_UNTIL.load(Ordering::SeqCst);
    if until == 0 {
        return false;
    }
    let cap = TAIL_CAP.load(Ordering::SeqCst);
    if t > until || t > cap {
        end_tail(if t > cap { Why::Capped } else { Why::Quiet });
        return false;
    }
    // a glide that goes on keeps the tail alive, up to its cap
    TAIL_UNTIL.store((t + TAIL_GAP_MS).min(cap).max(until), Ordering::SeqCst);
    DROPPED.fetch_add(1, Ordering::Relaxed);
    DROPPED_IN_TAIL.fetch_add(1, Ordering::Relaxed);
    diag::trace!("touch", "scroll_drop", tail = true);
    true
}

/// Whether the wheel should be grabbed (Linux): a hold runs, or a tail is still open. Also closes a
/// tail that ran out, which on Linux nobody else would notice (no wheel event arrives to see it).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn wanted() -> bool {
    if ACTIVE.load(Ordering::SeqCst) {
        return true;
    }
    let until = TAIL_UNTIL.load(Ordering::SeqCst);
    if until == 0 {
        return false;
    }
    let t = now_ms();
    let cap = TAIL_CAP.load(Ordering::SeqCst);
    if t > until || t > cap {
        end_tail(if t > cap { Why::Capped } else { Why::Quiet });
        return false;
    }
    true
}

/// The pie that a trackpad hold opened is over, `fingers` are still on the surface: the tail starts.
pub(crate) fn begin_tail(fingers: usize) {
    begin_tail_at(now_ms(), fingers);
}

fn begin_tail_at(t: u64, fingers: usize) {
    TAIL_FLOOR.store(fingers, Ordering::SeqCst);
    TAIL_CAP.store(t + TAIL_MAX_MS, Ordering::SeqCst);
    TAIL_UNTIL.store(t + TAIL_MS, Ordering::SeqCst);
    diag::log!(
        "touch",
        "scroll_tail",
        fingers = fingers,
        dropped = DROPPED.load(Ordering::Relaxed)
    );
}

/// A gesture that never opened ends: nothing was scrolled away, so there is nothing to finish. Logs
/// the swallowed events, if there were any.
pub(crate) fn end_without_tail() {
    let dropped = DROPPED.swap(0, Ordering::SeqCst);
    DROPPED_IN_TAIL.store(0, Ordering::SeqCst);
    if dropped > 0 {
        diag::log!("touch", "scroll_swallowed", dropped = dropped, tail = false);
    }
}

/// The number of fingers on the surface changed (called after every frame). A count above what the
/// tail started with is a new touch: the tail ends so the person's own scroll is never eaten.
pub(crate) fn on_fingers(count: usize) {
    if TAIL_UNTIL.load(Ordering::Relaxed) == 0 {
        return;
    }
    let floor = TAIL_FLOOR.load(Ordering::SeqCst);
    if count > floor {
        end_tail(Why::NewTouch);
    } else if count < floor {
        TAIL_FLOOR.store(count, Ordering::SeqCst);
    }
}

/// A new trackpad gesture begins (it owns the wheel itself).
pub(crate) fn on_next_gesture() {
    if TAIL_UNTIL.load(Ordering::SeqCst) != 0 {
        end_tail(Why::NextGesture);
    } else {
        DROPPED.store(0, Ordering::SeqCst);
        DROPPED_IN_TAIL.store(0, Ordering::SeqCst);
    }
}

/// The touch source stopped: no tail outlives it.
pub(crate) fn clear() {
    if TAIL_UNTIL.load(Ordering::SeqCst) != 0 {
        end_tail(Why::Stopped);
    }
    DROPPED.store(0, Ordering::SeqCst);
    DROPPED_IN_TAIL.store(0, Ordering::SeqCst);
}

/// Whether a tail is open (tests, and the log).
#[cfg(test)]
pub(crate) fn tail_open() -> bool {
    TAIL_UNTIL.load(Ordering::SeqCst) != 0
}

fn end_tail(why: Why) {
    // only the thread that closes it logs it
    if TAIL_UNTIL.swap(0, Ordering::SeqCst) == 0 {
        return;
    }
    TAIL_CAP.store(0, Ordering::SeqCst);
    let dropped = DROPPED.swap(0, Ordering::SeqCst);
    let in_tail = DROPPED_IN_TAIL.swap(0, Ordering::SeqCst);
    diag::log!(
        "touch",
        "scroll_tail_end",
        why = why.name(),
        dropped = dropped,
        in_tail = in_tail
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tap::TEST_SERIAL;
    use std::sync::MutexGuard;

    /// The statics are shared with the other tests of the touch source: one at a time.
    fn serial() -> MutexGuard<'static, ()> {
        let g = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        ACTIVE.store(false, Ordering::SeqCst);
        clear();
        g
    }

    #[test]
    fn nothing_is_swallowed_when_no_gesture_runs() {
        let _g = serial();
        assert!(!swallows());
        assert!(!swallows_at(5_000));
        assert!(!wanted());
    }

    #[test]
    fn every_wheel_event_is_swallowed_while_a_hold_runs() {
        let _g = serial();
        ACTIVE.store(true, Ordering::SeqCst);
        for t in [10, 20, 5_000] {
            assert!(swallows_at(t));
        }
        assert!(wanted());
        ACTIVE.store(false, Ordering::SeqCst);
        clear();
    }

    #[test]
    fn the_events_after_the_pie_closed_are_swallowed_too() {
        let _g = serial();
        // the pie closed at t = 1000 with the extra finger still down; its glide arrives after
        begin_tail_at(1_000, 1);
        assert!(tail_open());
        assert!(swallows_at(1_050), "the last deltas of the fingers");
        assert!(swallows_at(1_150), "the glide goes on");
        assert!(swallows_at(1_260));
        // quiet for longer than the gap: the tail is over, and the next scroll is the person's
        assert!(!swallows_at(1_260 + TAIL_GAP_MS + 1));
        assert!(!tail_open());
        assert!(!swallows_at(1_500), "a closed tail stays closed");
    }

    #[test]
    fn a_tail_with_no_event_ends_after_its_minimum() {
        let _g = serial();
        begin_tail_at(1_000, 0);
        assert!(swallows_at(1_000 + TAIL_MS));
        // that event extended it by the gap; now nothing comes
        assert!(!swallows_at(1_000 + TAIL_MS + TAIL_GAP_MS + 1));
        assert!(!tail_open());
    }

    #[test]
    fn a_glide_cannot_keep_the_tail_open_forever() {
        let _g = serial();
        begin_tail_at(1_000, 0);
        let mut t = 1_000;
        let mut last = true;
        // an event every 16 ms, like a long glide
        while last && t < 1_000 + 10 * TAIL_MAX_MS {
            t += 16;
            last = swallows_at(t);
        }
        assert!(!last, "the cap ended it");
        assert!(t <= 1_000 + TAIL_MAX_MS + 16, "ended by {t}");
        assert!(!tail_open());
    }

    #[test]
    fn a_new_finger_ends_the_tail_at_once() {
        let _g = serial();
        begin_tail_at(1_000, 1); // the extra finger is still down
        on_fingers(1);
        assert!(tail_open(), "the same fingers are no new touch");
        on_fingers(2); // a second finger lands: a scroll of the person's own
        assert!(!tail_open());
        assert!(!swallows_at(1_050), "their scroll reaches the app");
    }

    #[test]
    fn lifting_a_finger_then_touching_again_is_a_new_touch() {
        let _g = serial();
        begin_tail_at(1_000, 2);
        on_fingers(1);
        on_fingers(0);
        assert!(tail_open(), "lifting is not a new touch");
        on_fingers(1);
        assert!(
            !tail_open(),
            "one finger more than the lowest count since the end"
        );
    }

    #[test]
    fn a_hold_that_never_opened_has_no_tail() {
        let _g = serial();
        end_without_tail();
        assert!(!tail_open());
        assert!(!swallows_at(1_000));
    }

    #[test]
    fn the_next_gesture_and_a_stopped_source_close_the_tail() {
        let _g = serial();
        begin_tail_at(1_000, 0);
        on_next_gesture();
        assert!(!tail_open());
        begin_tail_at(2_000, 0);
        clear();
        assert!(!tail_open());
        assert!(!swallows_at(2_010));
    }

    #[test]
    fn the_tail_never_ends_before_it_started() {
        let _g = serial();
        // the clock read can be a hair behind the one that started the tail
        begin_tail_at(1_000, 0);
        assert!(swallows_at(999));
        assert!(tail_open());
    }
}
