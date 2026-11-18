//! Space while the pie is open opens Settings (the way into Settings for someone who turned off
//! "Hold to open settings").
//!
//! One or two presses of Space, as set in Behavior. The key is only taken while a pie is open and
//! the setting is on; in every other case it passes through untouched. A press that is taken is
//! swallowed together with its auto-repeat and its key-up, so it never reaches the app under the
//! pie. Counting is on the tap thread with a few atomics: nothing blocks.

use super::gesture::Gesture;
use super::{RECORDING, Verdict, trigger};
use crate::config::{cfg, lock};
use crate::keys::key;
use crate::msg::Notifier;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// How long after one press of Space the next one still counts toward the same series, in ms.
pub(crate) const WINDOW_MS: u64 = 500;

/// Space is down and CursorPie took its press (its auto-repeat and its key-up are swallowed too).
static HELD: AtomicBool = AtomicBool::new(false);
/// Presses counted in the current series.
static COUNT: AtomicU8 = AtomicU8::new(0);
/// When the last counted press happened, in ms since the tap's epoch, plus 1 (0 = none).
static LAST_MS: AtomicU64 = AtomicU64::new(0);

/// What a press does to a series: the new count, and whether Settings opens now. Pure, so it is
/// unit tested. `since_last_ms` is `None` for the first press (or after a pause too long for the
/// series); `needed` is the number of presses that open Settings (1 or 2).
pub(crate) fn step(count: u8, since_last_ms: Option<u64>, needed: u8) -> (u8, bool) {
    let continued = since_last_ms.is_some_and(|ms| ms <= WINDOW_MS);
    let now = if continued {
        count.saturating_add(1)
    } else {
        1
    };
    if now >= needed.max(1) {
        (0, true)
    } else {
        (now, false)
    }
}

/// Looks at one key event. `Some(verdict)` = Space was taken here and the verdict is final;
/// `None` = not ours, the normal key handling goes on.
pub(super) fn handle(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    down: bool,
    code: i32,
    mods: u8,
) -> Option<Verdict> {
    if code != i32::from(key::SPACE) {
        return None;
    }
    if !down {
        // the key-up of a press that was taken: swallowed, even if the pie is gone by now
        return HELD.swap(false, Ordering::SeqCst).then_some(Verdict::Drop);
    }
    if HELD.load(Ordering::SeqCst) {
        return Some(Verdict::Drop); // auto-repeat of a taken press: not a new one
    }
    if mods != 0 || RECORDING.load(Ordering::SeqCst) {
        return None;
    }
    let (on, needed) = {
        let c = cfg();
        (c.settings_by_space, c.settings_space_presses)
    };
    if !on || !matches!(*lock(state), Gesture::ActivePie { .. }) {
        COUNT.store(0, Ordering::SeqCst);
        return None;
    }
    HELD.store(true, Ordering::SeqCst);
    let now = super::now_ticks();
    let last = LAST_MS.swap(now, Ordering::SeqCst);
    let since = (last != 0).then(|| now.saturating_sub(last));
    let (count, open) = step(COUNT.load(Ordering::SeqCst), since, needed);
    COUNT.store(count, Ordering::SeqCst);
    if open {
        LAST_MS.store(0, Ordering::SeqCst);
        trigger::open_settings_from_pie(state, notifier);
    }
    Some(Verdict::Drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_press_opens_settings_when_one_is_needed() {
        assert_eq!(step(0, None, 1), (0, true));
        assert_eq!(step(0, Some(100), 1), (0, true));
    }

    #[test]
    fn two_presses_must_come_close_together() {
        assert_eq!(step(0, None, 2), (1, false), "the first one only counts");
        assert_eq!(step(1, Some(200), 2), (0, true), "the second one opens");
        assert_eq!(
            step(1, Some(WINDOW_MS + 1), 2),
            (1, false),
            "too late: it starts a new series"
        );
    }

    #[test]
    fn the_number_of_presses_is_never_zero() {
        assert_eq!(step(0, None, 0), (0, true));
    }
}
