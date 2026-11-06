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
use std::sync::{Mutex, MutexGuard};

/// The `MOD_*` bits held now, the one pressed first at the front.
static HELD: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn held() -> MutexGuard<'static, Vec<u8>> {
    HELD.lock().unwrap_or_else(|e| e.into_inner())
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
