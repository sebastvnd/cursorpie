//! The lock: CursorPie asleep, until the same gesture wakes it.
//!
//! Two ways lock it, and the lock is one for all of them:
//!
//! - **Three fingers** (trackpad): while the pie is open (one finger holds), two more fingers that
//!   tap together, three on the surface in all. Holding one finger and tapping with two more
//!   unlocks it. The recognizer only reports the gesture
//!   ([`TouchEvent::LockTap`](super::recognizer::TouchEvent::LockTap)); what it means (lock or
//!   unlock) is decided by the [`Bridge`](super::Bridge) from the state kept here.
//! - **Esc** (any trigger): Esc while a pie is open, whether the mouse button or a trackpad hold
//!   opened it. Holding the trigger while it is locked (a press of the mouse button, a hold on the
//!   trackpad) and pressing Esc unlocks it. See [`esc_lock`](crate::tap).
//!
//! While it is locked a hold opens no pie: a press of the mouse trigger goes to the app as if
//! CursorPie were not there, a trackpad hold does nothing, and both only say so, with a pill, once
//! the hold time has passed. The menu bar item is not touched by the lock.
//!
//! **Opening the lock while the trigger is still held opens the pie at once** (the padlock opens
//! in its middle): the hold that unlocked is the hold of the pie, nothing has to be triggered
//! again. On the trackpad two more fingers tap; with the mouse trigger Esc or a click of the left
//! button does it ([`esc_lock`](crate::tap)).
//!
//! The state is one flag in memory: it is not written to the config, so a restart of CursorPie is a
//! way out of a lock that the person forgot how to open.
//!
//! The settings [`Config::touch_lock`](crate::config::Config::touch_lock) (three fingers) and
//! [`Config::esc_lock`](crate::config::Config::esc_lock) (Esc) turn the ways on (both are on by
//! default). Turned off, two extra fingers break the hold as they did before the lock existed, and
//! a lock that was in force while both are off is forgotten.

use crate::config::cfg;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// CursorPie is locked.
static LOCKED: AtomicBool = AtomicBool::new(false);

/// A trigger is held down on a lock: a press of the mouse trigger or a hold on the trackpad that
/// opened no pie because of the lock, and that has not ended. Esc unlocks only while this is true,
/// so Esc stays the app's own key at every other time.
static HELD: AtomicBool = AtomicBool::new(false);

/// When (unix ms, plus 1; 0 = never) the lock was opened by a trigger that is still held, so that
/// the pie opens at once: the next pie that the UI is asked for carries the opening padlock (see
/// [`take_reveal`]).
static REVEAL_AT: AtomicU64 = AtomicU64::new(0);

/// A pie that was asked for within this time (ms) of [`mark_reveal`] is the one that came out of
/// the unlock; a later one is an ordinary pie.
const REVEAL_WINDOW_MS: u64 = 1500;

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
        + 1
}

/// The lock was opened while the trigger is held and a pie is about to open: say so before the
/// pie is asked for, so the UI can tell it from any other.
pub(crate) fn mark_reveal() {
    REVEAL_AT.store(unix_ms(), Ordering::SeqCst);
}

/// Forgets [`mark_reveal`] (the pie could not open after all).
pub(crate) fn clear_reveal() {
    REVEAL_AT.store(0, Ordering::SeqCst);
}

/// Whether the pie that is being shown came out of an unlock: true once per [`mark_reveal`], and
/// only when it is recent.
pub(crate) fn take_reveal() -> bool {
    let at = REVEAL_AT.swap(0, Ordering::SeqCst);
    at != 0 && unix_ms().saturating_sub(at) <= REVEAL_WINDOW_MS
}

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

/// Whether a trigger is held down on the lock (see [`HELD`]).
pub(crate) fn held() -> bool {
    HELD.load(Ordering::SeqCst)
}

/// Sets or clears [`held`].
pub(crate) fn set_held(held: bool) {
    HELD.store(held, Ordering::SeqCst);
}

/// Whether the lock is in force: locked, and a way to lock still switched on. A lock that outlived
/// the switches is cleared here, so switching a way on again later does not bring it back.
pub(crate) fn in_force() -> bool {
    if !is_locked() {
        return false;
    }
    let on = {
        let c = cfg();
        c.touch_lock || c.esc_lock
    };
    if on {
        true
    } else {
        set(false);
        false
    }
}
