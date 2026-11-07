//! Keeping macOS from putting CursorPie to sleep ("App Nap").
//!
//! CursorPie is an accessory app with no visible window almost all the time, which is exactly
//! what App Nap looks for. A napped app has its timers stretched out by seconds, its threads
//! moved to the slowest class and its memory first in line for compression. The first press after a long idle
//! time then waits for the hold timer, the main thread and the pages it needs all at once: the pie
//! shows late, or the press seems lost and has to be repeated.
//!
//! Declaring an activity opts the process out of that for as long as the activity lasts. The
//! options used keep the machine free to sleep (the display, the disks and the CPU are not held
//! awake), so the cost is only that the process runs at the priority of something the person
//! asked for, which a pie that must open when they press is.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSString;

/// `NSActivityUserInitiatedAllowingIdleSystemSleep`: `NSActivityUserInitiated` without
/// `NSActivityIdleSystemSleepDisabled`. User-initiated work is not napped and its timers are not
/// coalesced; the Mac may still sleep when it is idle.
const USER_INITIATED_ALLOWING_IDLE_SLEEP: u64 = 0x00FF_FFFF;

/// Opts the process out of App Nap for the rest of its life. Safe to call from any thread, once.
pub fn stay_responsive() {
    let reason = NSString::from_str("CursorPie listens for the pie trigger");
    // SAFETY: `+[NSProcessInfo processInfo]` takes no arguments and returns the shared instance.
    // `-beginActivityWithOptions:reason:` takes an `NSActivityOptions` (an unsigned integer) and an
    // `NSString`, and returns the opaque token of the activity.
    let token: Option<Retained<AnyObject>> = unsafe {
        let info: Option<Retained<AnyObject>> = msg_send![class!(NSProcessInfo), processInfo];
        let Some(info) = info else {
            return;
        };
        msg_send![
            &*info,
            beginActivityWithOptions: USER_INITIATED_ALLOWING_IDLE_SLEEP,
            reason: &*reason
        ]
    };
    let held = token.is_some();
    // The activity ends when its token is released, so the token is kept on purpose, for good.
    if let Some(token) = token {
        std::mem::forget(token);
    }
    diag::log!("sys", "app_nap", off = held);
}
