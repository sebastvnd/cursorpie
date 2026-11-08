//! While a pie is open, no click and no scroll reaches the app under it.
//!
//! Each path of the state machine decides about its own buttons: the trigger runs the pie, the right
//! button switches it, a mapped side button runs an action, the extra finger of a trackpad hold
//! clicks it. Every other press (the left button, a right button that is not switching, a middle or
//! extra button no mode uses) used to fall through as `Keep`: the app under the open pie got a click
//! that the person aimed at the pie. On Linux it was worse, a grabbed press that was kept is sent
//! again to the app at its release.
//!
//! The rule here is the last word on an event the other paths let through: **while a pie is open
//! (`Gesture::ActivePie`, whoever opened it) a press is swallowed, and so are its drags and its
//! release**, even when the pie closed in between (the release is the same click's). Every press
//! decides afresh, so a release that was lost never leaves the next click stuck.
//!
//! The wheel follows the same rule for a mouse pie ([`swallows_wheel`]; a trackpad pie has its own
//! longer one, see `touch::scroll`).
//!
//! **Cost.** With no pie open and nothing swallowed, the native taps ask [`interested`]: one atomic
//! load and, for a press, one `try_lock` that never waits.

use super::{Gesture, Kind, Verdict};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, TryLockError};

/// The buttons (bit = macOS button number) whose press was swallowed and whose release is still due.
static HELD: AtomicU32 = AtomicU32::new(0);

fn bit(button: i64) -> Option<u32> {
    (0..32).contains(&button).then(|| 1u32 << button)
}

/// Whether a pie is open right now. Never waits for the lock: a contended state counts as "no", so
/// a hook is never stalled (the event passes, as before).
pub(crate) fn pie_open(state: &Arc<Mutex<Gesture>>) -> bool {
    match state.try_lock() {
        Ok(g) => matches!(*g, Gesture::ActivePie { .. }),
        Err(TryLockError::Poisoned(e)) => matches!(*e.into_inner(), Gesture::ActivePie { .. }),
        Err(TryLockError::WouldBlock) => false,
    }
}

/// Whether the wheel is swallowed because a pie is open (the part of the rule the wheel has).
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub(crate) fn swallows_wheel(state: &Arc<Mutex<Gesture>>) -> bool {
    pie_open(state)
}

/// Whether a left-button event is worth the native tap reading it (the other buttons always are):
/// a press while a pie is open, or a release or drag of a click that was swallowed.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub(crate) fn interested(state: &Arc<Mutex<Gesture>>, kind: Kind) -> bool {
    HELD.load(Ordering::Relaxed) != 0 || (kind == Kind::Down && pie_open(state))
}

/// The last word on `verdict`, what the other paths decided for this event.
pub(super) fn filter(
    state: &Arc<Mutex<Gesture>>,
    kind: Kind,
    button: i64,
    verdict: Verdict,
) -> Verdict {
    let Some(b) = bit(button) else {
        return verdict;
    };
    match kind {
        Kind::Down => {
            // every press decides afresh
            HELD.fetch_and(!b, Ordering::SeqCst);
            if verdict == Verdict::Keep && pie_open(state) {
                HELD.fetch_or(b, Ordering::SeqCst);
                diag::log!("tap", "pie_click_swallowed", button = button);
                return Verdict::Drop;
            }
            verdict
        }
        Kind::Up => {
            if HELD.fetch_and(!b, Ordering::SeqCst) & b != 0 {
                Verdict::Drop
            } else {
                verdict
            }
        }
        Kind::Drag => {
            if HELD.load(Ordering::Relaxed) & b != 0 {
                Verdict::Drop
            } else {
                verdict
            }
        }
    }
}

/// Forgets every swallowed press (the tap was disabled by the system, or the source stopped).
pub(crate) fn clear() {
    HELD.store(0, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;
    use crate::tap::TEST_SERIAL;
    use crate::tap::gesture::Source;

    fn open(source: Source) -> Arc<Mutex<Gesture>> {
        Arc::new(Mutex::new(Gesture::ActivePie {
            id: 1,
            center: Point::new(0.0, 0.0),
            profile: 0,
            path: Vec::new(),
            hover: None,
            source,
        }))
    }

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        let g = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        g
    }

    #[test]
    fn a_click_while_a_pie_is_open_never_reaches_the_app() {
        let _g = serial();
        for source in [Source::Mouse, Source::Touch] {
            let st = open(source);
            for button in [0, 1, 2, 5] {
                assert_eq!(
                    filter(&st, Kind::Down, button, Verdict::Keep),
                    Verdict::Drop
                );
                assert_eq!(
                    filter(&st, Kind::Drag, button, Verdict::Keep),
                    Verdict::Drop
                );
                assert_eq!(filter(&st, Kind::Up, button, Verdict::Keep), Verdict::Drop);
                assert_eq!(HELD.load(Ordering::SeqCst), 0, "the release ends it");
            }
        }
    }

    #[test]
    fn the_release_of_a_swallowed_click_is_swallowed_after_the_pie_closed() {
        let _g = serial();
        let st = open(Source::Mouse);
        assert_eq!(filter(&st, Kind::Down, 0, Verdict::Keep), Verdict::Drop);
        *crate::config::lock(&st) = Gesture::Idle; // the pie closed while the button was down
        assert_eq!(filter(&st, Kind::Up, 0, Verdict::Keep), Verdict::Drop);
        // and the next click is the app's again
        assert_eq!(filter(&st, Kind::Down, 0, Verdict::Keep), Verdict::Keep);
        assert_eq!(filter(&st, Kind::Up, 0, Verdict::Keep), Verdict::Keep);
    }

    #[test]
    fn with_no_pie_open_clicks_pass() {
        let _g = serial();
        let st = Arc::new(Mutex::new(Gesture::Idle));
        for kind in [Kind::Down, Kind::Drag, Kind::Up] {
            assert_eq!(filter(&st, kind, 0, Verdict::Keep), Verdict::Keep);
        }
        assert!(!interested(&st, Kind::Down));
        assert!(!swallows_wheel(&st));
    }

    #[test]
    fn a_lost_release_never_leaves_the_next_click_stuck() {
        let _g = serial();
        let st = open(Source::Touch);
        assert_eq!(filter(&st, Kind::Down, 0, Verdict::Keep), Verdict::Drop);
        *crate::config::lock(&st) = Gesture::Idle; // the release never came
        assert_eq!(filter(&st, Kind::Down, 0, Verdict::Keep), Verdict::Keep);
        assert_eq!(filter(&st, Kind::Drag, 0, Verdict::Keep), Verdict::Keep);
        assert_eq!(HELD.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn what_another_path_already_swallowed_is_left_to_it() {
        let _g = serial();
        let st = open(Source::Mouse);
        assert_eq!(filter(&st, Kind::Down, 1, Verdict::Drop), Verdict::Drop);
        assert_eq!(HELD.load(Ordering::SeqCst), 0, "not ours to remember");
        assert_eq!(filter(&st, Kind::Up, 1, Verdict::Keep), Verdict::Keep);
    }

    #[test]
    fn the_wheel_is_swallowed_while_a_pie_is_open() {
        let _g = serial();
        assert!(swallows_wheel(&open(Source::Mouse)));
        assert!(swallows_wheel(&open(Source::Touch)));
    }
}
