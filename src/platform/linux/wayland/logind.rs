//! Watches the login session through systemd-logind, so CursorPie lets go of the mouse while the
//! screen is locked or the session is not the one on screen (a switch to another VT or user).
//!
//! Why it matters: the Wayland layer grabs the mouse. A grab that outlived the lock screen would
//! swallow the trigger button over the password prompt, and over another user's session. While the
//! session is "away" the layer lifts every grab (`tap::wayland`, the same path the pause switch
//! takes), and takes them back when the session is on screen and unlocked again.
//!
//! Fail open: if logind cannot be reached the answer is "not away", so a machine without logind
//! (or without a session bus' system twin) is never left paused by this module.
//!
//! The decision is pure ([`away`]); the thread only reads two properties of
//! `org.freedesktop.login1.Session` every [`EVERY`]: `Active` (this session is in front) and
//! `LockedHint` (the lock screen asked for the hint, which KDE's screen locker does).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use zbus::blocking::Connection;
use zbus::blocking::fdo::PropertiesProxy;
use zbus::names::InterfaceName;

const SERVICE: &str = "org.freedesktop.login1";
/// The session of the caller; logind resolves `auto` itself.
const PATH: &str = "/org/freedesktop/login1/session/auto";
const IFACE: &str = "org.freedesktop.login1.Session";
/// How often the two properties are read.
const EVERY: Duration = Duration::from_millis(500);
/// How long to wait before asking again after a failure.
const RETRY: Duration = Duration::from_secs(5);

static AWAY: AtomicBool = AtomicBool::new(false);

/// Whether the session is away: locked, or not the active one. Pure.
pub(crate) fn away(active: bool, locked: bool) -> bool {
    !active || locked
}

/// The last answer: the grabs must be lifted while this is `true`.
pub(crate) fn is_away() -> bool {
    AWAY.load(Ordering::Relaxed)
}

fn set(now: bool) {
    if AWAY.swap(now, Ordering::Relaxed) != now {
        diag::log!("wayland", "session_away", away = now);
    }
}

fn read_bool(props: &PropertiesProxy<'_>, name: &str) -> Option<bool> {
    let iface = InterfaceName::try_from(IFACE).ok()?;
    let value = props.get(iface, name).ok()?;
    bool::try_from(value).ok()
}

/// Starts the thread (once).
pub(crate) fn start() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("cursorpie-logind".into())
        .spawn(|| {
            loop {
                let props = Connection::system().ok().and_then(|conn| {
                    PropertiesProxy::builder(&conn)
                        .destination(SERVICE)
                        .ok()?
                        .path(PATH)
                        .ok()?
                        .build()
                        .ok()
                });
                let Some(props) = props else {
                    set(false);
                    std::thread::sleep(RETRY);
                    continue;
                };
                loop {
                    let (Some(active), Some(locked)) =
                        (read_bool(&props, "Active"), read_bool(&props, "LockedHint"))
                    else {
                        // logind went away or this process has no session: never stay paused
                        set(false);
                        break;
                    };
                    set(away(active, locked));
                    std::thread::sleep(EVERY);
                }
                std::thread::sleep(RETRY);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_or_background_session_is_away() {
        assert!(!away(true, false), "on screen and unlocked");
        assert!(away(true, true), "lock screen");
        assert!(away(false, false), "another VT or user in front");
        assert!(away(false, true));
    }
}
