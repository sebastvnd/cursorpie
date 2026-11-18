//! Is there a trackpad? A look at the system that starts nothing.
//!
//! The touch source of every OS idles until `touch_trigger` is on, and only a running source knows
//! whether a trackpad exists. That is a circle for a first start: the switch is off because it is
//! opt-in, so nothing can say that it should be on. This module breaks the circle with a count
//! that needs neither the private framework (macOS), nor a Raw Input registration (Windows), nor
//! permission to read `/dev/input` (Linux):
//!
//! | OS | What is asked | Needs |
//! |---|---|---|
//! | Windows | the Raw Input device list, the same precision-touchpad test the source uses | nothing, no registration |
//! | Linux | `/proc/bus/input/devices` (readable by everyone), the same parser the source uses | nothing, not the `input` group |
//! | macOS | `ioreg` for the trackpad classes of the I/O registry | nothing, the private framework stays unloaded |
//!
//! The count is what a **scan** (the button of the Trackpad panel, "Detect again" of the Mouse
//! panel) and **every start** use: with a trackpad present the hold is switched on once, so nobody
//! has to find the switch (see [`auto_enable_on_first_start`]).

use crate::config::{cfg, save_config};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// "No scan yet" in [`LAST`].
const NEVER: usize = usize::MAX;
/// What the last scan counted.
static LAST: AtomicUsize = AtomicUsize::new(NEVER);
/// The trackpad hold was switched on by the first start and `config.json` has not been written
/// since (see [`take_pending_save`]).
static PENDING_SAVE: AtomicBool = AtomicBool::new(false);

/// How many objects of class `class` an `ioreg -r -c <class>` listing shows. Pure, so it is unit
/// tested on every OS. A registry entry is a line like
/// `+-o AppleMultitouchTrackpad  <class AppleMultitouchTrackpad, id 0x1000003a5, registered, ...>`.
#[allow(dead_code)] // read by the macOS backend (and by tests)
pub(crate) fn count_ioreg(listing: &str, class: &str) -> usize {
    let tag = format!("<class {class},");
    listing
        .lines()
        .filter(|l| l.contains("+-o ") && l.contains(&tag))
        .count()
}

/// How many trackpads the system lists right now. Starts nothing, loads nothing private.
pub fn probe() -> usize {
    crate::platform::probe_trackpads()
}

/// Looks for trackpads now and remembers the answer for Settings ([`last_scan`]).
pub fn scan() -> usize {
    let n = probe();
    LAST.store(n, Ordering::SeqCst);
    diag::log!("touch", "scan", trackpads = n);
    n
}

/// What the last [`scan`] counted; `None` before the first one.
pub fn last_scan() -> Option<usize> {
    match LAST.load(Ordering::SeqCst) {
        NEVER => None,
        n => Some(n),
    }
}

/// The decision of a start, from the facts it depends on. Pure, so it is unit tested.
///
/// `done` is [`crate::config::Config::touch_auto_done`]: once the hold was switched on for a found
/// trackpad, or the person decided in Settings, the choice is theirs and is never made for them again.
pub fn should_auto_enable(done: bool, trackpads: usize, already_on: bool) -> bool {
    !done && trackpads > 0 && !already_on
}

/// Every start: when a trackpad is present and nobody decided yet, the trackpad hold is switched
/// on, so nobody has to find the switch. Returns whether it switched it on.
///
/// This used to be the first start of a new user only. Now any start that finds a trackpad does it
/// once (a Bluetooth trackpad, a docked laptop, or a copy that was installed before this existed);
/// without a trackpad nothing is decided, so the first one that shows up is picked up at the next
/// start. After that [`crate::config::Config::touch_auto_done`] is set and the switch in Settings
/// is the only thing that changes the hold.
///
/// On the first start of a new user the switch is changed in memory here, before the tap starts, and
/// written to disk by [`take_pending_save`] when the welcome window ends: writing `config.json` now
/// would make the next start look like an existing user's and skip the welcome window if the app was
/// quit early. Any other start writes it at once.
pub fn auto_enable_on_first_start(first_start: bool) -> bool {
    if cfg().touch_auto_done {
        return false;
    }
    let n = scan();
    if n == 0 {
        diag::log!("touch", "auto_enable", trackpads = n, enabled = false);
        return false;
    }
    let (enable, snapshot) = {
        let mut c = cfg();
        let go = should_auto_enable(c.touch_auto_done, n, c.touch_trigger);
        if go {
            c.touch_trigger = true;
        }
        c.touch_auto_done = true;
        (go, c.clone())
    };
    if first_start {
        PENDING_SAVE.store(true, Ordering::SeqCst);
    } else {
        save_config(&snapshot);
    }
    diag::log!("touch", "auto_enable", trackpads = n, enabled = enable);
    enable
}

/// Writes the config once if the first start switched the trackpad hold on. Called when the
/// welcome window ends (the one moment a new user's `config.json` is written).
pub fn take_pending_save() {
    if PENDING_SAVE.swap(false, Ordering::SeqCst) {
        let snapshot = cfg().clone();
        save_config(&snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trackpad_is_switched_on_once_when_one_is_found() {
        assert!(should_auto_enable(false, 1, false));
        assert!(should_auto_enable(false, 3, false));
        assert!(
            !should_auto_enable(false, 0, false),
            "no trackpad, nothing to turn on"
        );
        assert!(
            !should_auto_enable(true, 1, false),
            "a decided switch is never changed again"
        );
        assert!(!should_auto_enable(false, 1, true), "already on");
    }

    #[test]
    fn ioreg_listings_are_counted_by_class() {
        let one = "+-o AppleMultitouchTrackpad  <class AppleMultitouchTrackpad, id 0x1000003a5, registered, matched, active, busy 0 (0 ms), retain 8>\n    {\n      \"Built-In\" = Yes\n    }\n";
        assert_eq!(count_ioreg(one, "AppleMultitouchTrackpad"), 1);
        assert_eq!(count_ioreg(one, "BNBTrackpadDevice"), 0);
        let two = format!("{one}\n{}", one.replace("0x1000003a5", "0x1000004b7"));
        assert_eq!(count_ioreg(&two, "AppleMultitouchTrackpad"), 2);
        assert_eq!(count_ioreg("", "AppleMultitouchTrackpad"), 0);
        // a property that only mentions the class is not an entry of the registry
        let prop = "    \"IOClass\" = \"<class AppleMultitouchTrackpad,\"\n";
        assert_eq!(count_ioreg(prop, "AppleMultitouchTrackpad"), 0);
    }

    #[test]
    fn a_scan_is_remembered_for_settings() {
        let n = scan();
        assert_eq!(last_scan(), Some(n));
    }

    #[test]
    fn a_decided_switch_is_never_touched() {
        cfg().touch_auto_done = true;
        assert!(!auto_enable_on_first_start(false));
        cfg().touch_auto_done = false;
    }
}
