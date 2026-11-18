//! The order in which the modifier keys are being held, so a recorded shortcut keeps the order the
//! user pressed them in (Cmd, Shift, Opt, then W shows as "Cmd + Shift + Opt + W").
//!
//! The key events of the hooks carry only a bitmask of the modifiers held, never the order. Each
//! native layer therefore reports every modifier change here: `press` / `release` when it sees
//! the key itself (Windows, Linux), `sync` when it only gets the new set of flags (macOS). The
//! recorder then asks [`ordered`] for the order that fits the mask of the key it captured.

// Each OS uses a different part of this API.
#![allow(dead_code)]

use crate::keys::MOD_ORDER;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, MutexGuard};

/// The `MOD_*` bits held now, the one pressed first at the front.
static HELD: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn held() -> MutexGuard<'static, Vec<u8>> {
    HELD.lock().unwrap_or_else(|e| e.into_inner())
}

/// The Windows keys the hook is swallowing right now: [`WIN_LEFT`] and [`WIN_RIGHT`] bits, set when
/// the press was swallowed and cleared when its release has been swallowed too.
static WIN_SWALLOWED: AtomicU8 = AtomicU8::new(0);

/// The left Windows key in [`win_event`].
pub(super) const WIN_LEFT: u8 = 1;
/// The right Windows key in [`win_event`].
pub(super) const WIN_RIGHT: u8 = 2;

/// A Windows key (`side` is [`WIN_LEFT`] or [`WIN_RIGHT`]) went down or up. Returns `true` when the
/// hook must swallow the event.
///
/// While a shortcut is being recorded the Windows key is taken for the recorder, so Windows never
/// sees it: an unswallowed press makes the shell open the Start menu when the key is let go (and
/// turns Win+key into the shell's own shortcut). Once a press was swallowed, its repeats and its
/// release are swallowed too, even after the recording is over, so the shell never gets half a
/// press. Outside a recording the key passes through untouched.
pub(super) fn win_event(side: u8, down: bool, recording: bool) -> bool {
    if down {
        if recording || WIN_SWALLOWED.load(Ordering::SeqCst) & side != 0 {
            WIN_SWALLOWED.fetch_or(side, Ordering::SeqCst);
            return true;
        }
        false
    } else {
        WIN_SWALLOWED.fetch_and(!side, Ordering::SeqCst) & side != 0
    }
}

/// Whether a Windows key is held that the hook swallowed (Windows itself does not know it is down).
pub(super) fn win_swallowed() -> bool {
    WIN_SWALLOWED.load(Ordering::SeqCst) != 0
}

/// A modifier went down. A repeat of a key that is already held keeps its place.
pub(super) fn press(bit: u8) {
    let mut h = held();
    if !h.contains(&bit) {
        h.push(bit);
    }
}

/// A modifier went up.
pub(super) fn release(bit: u8) {
    held().retain(|b| *b != bit);
}

/// The set of modifiers is now exactly `mods`: drops the ones that went up and appends the ones that
/// went down (in the usual order if several came at once).
pub(super) fn sync(mods: u8) {
    let mut h = held();
    h.retain(|b| mods & b != 0);
    for bit in MOD_ORDER {
        if mods & bit != 0 && !h.contains(&bit) {
            h.push(bit);
        }
    }
}

/// The modifiers of `mods` in the order they were pressed. A hook that missed a release or a press
/// is corrected by `mods`, which is always the truth.
pub(super) fn ordered(mods: u8) -> Vec<u8> {
    sync(mods);
    held().clone()
}

/// Forgets everything held.
pub(super) fn clear() {
    held().clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{MOD_ALT, MOD_CMD, MOD_CTRL, MOD_SHIFT};

    fn serial() -> MutexGuard<'static, ()> {
        super::super::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn keeps_the_press_order() {
        let _s = serial();
        clear();
        press(MOD_CMD);
        press(MOD_SHIFT);
        press(MOD_ALT);
        press(MOD_CMD); // key repeat
        assert_eq!(
            ordered(MOD_CMD | MOD_SHIFT | MOD_ALT),
            vec![MOD_CMD, MOD_SHIFT, MOD_ALT]
        );
        release(MOD_SHIFT);
        assert_eq!(ordered(MOD_CMD | MOD_ALT), vec![MOD_CMD, MOD_ALT]);
        clear();
    }

    #[test]
    fn the_windows_key_is_swallowed_while_recording_and_until_it_is_released() {
        let _s = serial();
        WIN_SWALLOWED.store(0, Ordering::SeqCst);
        // not recording: the key reaches Windows
        assert!(!win_event(WIN_LEFT, true, false));
        assert!(!win_event(WIN_LEFT, false, false));
        assert!(!win_swallowed());
        // recording: press and repeat are swallowed, and so is the release after the recording
        assert!(win_event(WIN_LEFT, true, true));
        assert!(win_swallowed());
        assert!(win_event(WIN_LEFT, true, false), "key repeat");
        assert!(win_event(WIN_LEFT, false, false), "release");
        assert!(!win_swallowed());
        // a release of a key that was never swallowed passes
        assert!(!win_event(WIN_RIGHT, false, true));
        // each side is tracked on its own
        assert!(win_event(WIN_RIGHT, true, true));
        assert!(!win_event(WIN_LEFT, true, false));
        assert!(win_event(WIN_RIGHT, false, false));
        assert!(!win_swallowed());
    }

    #[test]
    fn the_mask_corrects_a_missed_event() {
        let _s = serial();
        clear();
        press(MOD_CMD);
        // Cmd was released without us noticing, Ctrl was pressed without us noticing
        assert_eq!(ordered(MOD_CTRL), vec![MOD_CTRL]);
        clear();
    }

    #[test]
    fn sync_follows_a_flags_changed_stream() {
        let _s = serial();
        clear();
        sync(MOD_CMD);
        sync(MOD_CMD | MOD_SHIFT);
        sync(MOD_CMD | MOD_SHIFT | MOD_ALT);
        assert_eq!(
            ordered(MOD_CMD | MOD_SHIFT | MOD_ALT),
            vec![MOD_CMD, MOD_SHIFT, MOD_ALT]
        );
        clear();
    }
}
