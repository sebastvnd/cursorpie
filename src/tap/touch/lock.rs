//! The lock: CursorPie asleep for the trackpad, until the same gesture wakes it.
//!
//! While the pie is open (one finger holds), two more fingers that tap together, three on the
//! surface in all, lock CursorPie. While it is locked a hold opens no pie: it only says so, with a
//! pill, once the hold time has passed. Holding one finger and tapping with two more unlocks it.
//!
//! The recognizer only reports the gesture ([`TouchEvent::LockTap`](super::recognizer::TouchEvent::LockTap));
//! what it means (lock or unlock) is decided by the [`Bridge`](super::Bridge) from the state kept
//! here. The state is one flag in memory: it is not written to the config, so a restart of CursorPie
//! is a way out of a lock that the person forgot how to open. It locks the *trackpad* trigger only;
//! the mouse trigger and the menu bar item are not touched by it.
//!
//! The setting [`Config::touch_lock`](crate::config::Config::touch_lock) turns the gesture on (the
//! default). Turned off, two extra fingers break the hold as they did before the lock existed, and a
//! lock that was in force is forgotten.

use crate::config::cfg;
use std::sync::atomic::{AtomicBool, Ordering};

/// CursorPie is locked for the trackpad.
static LOCKED: AtomicBool = AtomicBool::new(false);

/// Fingers on the surface during the lock gesture: the holding one and the two that tap.
pub(crate) const LOCK_FINGERS: usize = 3;

/// Whether the flag is set (one atomic read, for the click path; see [`in_force`] for the decision).
pub(crate) fn is_locked() -> bool {
    LOCKED.load(Ordering::SeqCst)
}

/// Sets or clears the lock.
pub(crate) fn set(locked: bool) {
    LOCKED.store(locked, Ordering::SeqCst);
}

/// Whether the lock is in force: locked, and the gesture still switched on. A lock that outlived the
/// switch is cleared here, so switching the gesture on again later does not bring it back.
pub(crate) fn in_force() -> bool {
    if !is_locked() {
        return false;
    }
    if cfg().touch_lock {
        true
    } else {
        set(false);
        false
    }
}
