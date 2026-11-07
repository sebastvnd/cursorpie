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
//! panel) and the **first start** use: with a trackpad present the hold is switched on for a new
//! user, so nobody has to find the switch (see [`auto_enable_on_first_start`]).

use crate::config::{cfg, save_config};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// "No scan yet" in [`LAST`].
const NEVER: usize = usize::MAX;
/// What the last scan counted.
static LAST: AtomicUsize = AtomicUsize::new(NEVER);
/// The trackpad hold was switched on by the first start and `config.json` has not been written
/// since (see [`take_pending_save`]).
static PENDING_SAVE: AtomicBool = AtomicBool::new(false);

/// The trackpad classes of the macOS I/O registry: the built-in one and a Bluetooth Magic Trackpad.
/// (`AppleMultitouchDevice` is left out on purpose: a Magic Mouse is one too.)
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MAC_CLASSES: [&str; 2] = ["AppleMultitouchTrackpad", "BNBTrackpadDevice"];

/// How many objects of class `class` an `ioreg -r -c <class>` listing shows. Pure, so it is unit
/// tested on every OS. A registry entry is a line like
/// `+-o AppleMultitouchTrackpad  <class AppleMultitouchTrackpad, id 0x1000003a5, registered, ...>`.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub(crate) fn count_ioreg(listing: &str, class: &str) -> usize {
    let tag = format!("<class {class},");
    listing
        .lines()
        .filter(|l| l.contains("+-o ") && l.contains(&tag))
        .count()
}

/// Asks `ioreg` for one class, with a time limit. `None` when it could not be run or answered late.
#[cfg(target_os = "macos")]
fn ioreg(class: &str) -> Option<String> {
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Duration;
    let class = class.to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("cursorpie-ioreg".into())
        .spawn(move || {
            let out = Command::new("/usr/sbin/ioreg")
                .args(["-r", "-c", &class, "-d", "0"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
            let _ = tx.send(out);
        })
        .ok()?;
    rx.recv_timeout(Duration::from_millis(1500)).ok().flatten()
}

/// How many trackpads the system lists right now. Starts nothing, loads nothing private.
pub fn probe() -> usize {
    #[cfg(target_os = "macos")]
    {
        // one physical trackpad can show up under both classes: the larger count, not the sum
        MAC_CLASSES
            .iter()
            .filter_map(|c| ioreg(c).map(|l| count_ioreg(&l, c)))
            .max()
            .unwrap_or(0)
    }
    #[cfg(all(windows, not(target_os = "macos")))]
    {
        super::precision::probe()
    }
    // `check-windows` on another OS only type-checks the Windows code: its Raw Input calls do not
    // exist there, so they must not be reachable, or the test binary fails to link.
    #[cfg(all(feature = "check-windows", not(windows), not(target_os = "macos")))]
    {
        0
    }
    #[cfg(all(
        target_os = "linux",
        not(feature = "check-windows"),
        not(target_os = "macos")
    ))]
    {
        super::evdev::probe()
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        windows,
        feature = "check-windows"
    )))]
    {
        0
    }
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

/// The decision of the first start, from the three facts it depends on. Pure, so it is unit tested.
pub fn should_auto_enable(first_start: bool, trackpads: usize, already_on: bool) -> bool {
    first_start && trackpads > 0 && !already_on
}

/// First start of a new user: when a trackpad is present, the trackpad hold is switched on, so
/// nobody has to find the switch. Returns whether it did.
///
/// Only a *new* user is touched. Someone who already has a `config.json` made the choice (the
/// switch is off by default) and keeps it; a scan in Settings is how they turn it on later. The
/// switch is changed in memory here, before the tap starts, and written to disk by
/// [`take_pending_save`] when the welcome window ends: writing `config.json` now would make the
/// next start look like an existing user's and skip the welcome window if the app was quit early.
pub fn auto_enable_on_first_start(first_start: bool) -> bool {
    if !first_start {
        return false;
    }
    let n = scan();
    let enable = {
        let mut c = cfg();
        let go = should_auto_enable(first_start, n, c.touch_trigger);
        if go {
            c.touch_trigger = true;
        }
        go
    };
    if enable {
        PENDING_SAVE.store(true, Ordering::SeqCst);
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
    fn a_trackpad_is_switched_on_only_for_a_new_user_who_has_one() {
        assert!(should_auto_enable(true, 1, false));
        assert!(should_auto_enable(true, 3, false));
        assert!(
            !should_auto_enable(true, 0, false),
            "no trackpad, nothing to turn on"
        );
        assert!(
            !should_auto_enable(false, 1, false),
            "an existing user keeps their choice"
        );
        assert!(!should_auto_enable(true, 1, true), "already on");
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
    fn an_existing_user_is_never_touched() {
        assert!(!auto_enable_on_first_start(false));
    }
}
