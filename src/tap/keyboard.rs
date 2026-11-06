//! Key events: only used to record a shortcut for the settings window.

use super::{RECORDING, SWALLOW_KEY, Verdict};
use crate::keys::Shortcut;
use crate::msg::{Msg, Notifier};
use std::sync::atomic::Ordering;

/// Captures the next key press while Settings is recording a shortcut (plain Esc cancels).
/// The key is swallowed until its key-up so it does not leak to the app in front. Outside
/// recording every key event passes through untouched. `code` is a macOS virtual key code (the
/// native layer translates), `mods` a `MOD_*` bitmask.
pub(super) fn handle_key(notifier: &Notifier, down: bool, code: i32, mods: u8) -> Verdict {
    let swallow = SWALLOW_KEY.load(Ordering::SeqCst);

    if !down {
        if swallow == code {
            SWALLOW_KEY.store(-1, Ordering::SeqCst);
            return Verdict::Drop;
        }
        return Verdict::Keep;
    }

    // key down (including auto-repeat of the key that was just recorded)
    if swallow == code {
        return Verdict::Drop;
    }
    if !RECORDING.load(Ordering::SeqCst) {
        return Verdict::Keep;
    }

    RECORDING.store(false, Ordering::SeqCst);
    SWALLOW_KEY.store(code, Ordering::SeqCst);
    if code == 53 && mods == 0 {
        notifier.send(Msg::RecordCancelled); // plain Esc cancels
    } else {
        // the order the modifiers were pressed in, so the shortcut reads as it was typed
        let pressed = super::modorder::ordered(mods);
        notifier.send(Msg::KeyCaptured(
            Shortcut::new(code as u16, mods).with_order(&pressed),
        ));
    }
    Verdict::Drop
}
