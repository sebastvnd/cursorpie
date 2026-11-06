//! "Launch at login" through the per-user `Run` registry key.
//!
//! The entry always points at the installed copy (see [`install`](super::install)), never at
//! wherever the program happened to be started from: a download folder or an unzipped archive can
//! be moved or emptied, and then nothing would start at the next login.

use super::install::{same_file, startup_exe};
use super::registry;
use super::util::{
    STARTUP_ENABLED, exe_of_run_value, run_value, startup_entry_disabled, utf16_bytes_to_string,
};
use crate::about::{APP_NAME, AUTOSTART_ARG};
use std::path::Path;

/// The key every per-user startup entry lives in.
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Where Windows keeps the "switched off in Task Manager" flag of each startup entry.
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// The command stored in the `Run` entry, when the entry exists and the user has not switched it
/// off in Task Manager (Windows then keeps the entry but never runs it).
fn active_entry() -> Option<String> {
    let value = utf16_bytes_to_string(&registry::get_bytes(RUN_KEY, APP_NAME)?);
    let off =
        registry::get_bytes(APPROVED_KEY, APP_NAME).is_some_and(|d| startup_entry_disabled(&d));
    (!off).then_some(value)
}

/// Whether CursorPie starts with Windows: the entry exists, the user has not switched it off in
/// Task Manager, and the program it names is still there. An entry that points at a file that was
/// moved or deleted starts nothing, so it must not show a checked box.
pub fn launch_at_login_enabled() -> bool {
    active_entry()
        .and_then(|v| exe_of_run_value(&v))
        .is_some_and(|exe| Path::new(&exe).is_file())
}

/// Repairs an entry that cannot work: it names a program that is gone (the folder was moved or
/// cleaned up), or it is an old one without `--autostart` (the program would then not wait for the
/// desktop at login). Does nothing when there is no entry or the user switched it off, and nothing
/// when the entry is fine, so the registry is not written at every start.
pub fn repair_launch_entry() {
    let Some(value) = active_entry() else {
        return;
    };
    let broken = exe_of_run_value(&value).is_none_or(|exe| !Path::new(&exe).is_file());
    let outdated = !value.contains(AUTOSTART_ARG);
    // an entry that names another existing copy (a build the user chose on purpose) is left alone
    let elsewhere = exe_of_run_value(&value).is_some_and(|exe| {
        std::env::current_exe().is_ok_and(|me| !same_file(Path::new(&exe), &startup_exe(&me)))
    });
    if (broken || (outdated && !elsewhere))
        && let Err(e) = set_launch_at_login(true)
    {
        eprintln!("CursorPie: could not repair the launch at login entry: {e}");
    }
}

/// Adds or removes the startup entry. It takes effect at the next login (no second copy is started now).
///
/// Turning it on also clears a "disabled" flag left by Task Manager, otherwise the checkbox would
/// say yes while Windows still skipped the entry.
pub fn set_launch_at_login(on: bool) -> std::io::Result<()> {
    if !on {
        return registry::delete_value(RUN_KEY, APP_NAME);
    }
    let exe = startup_exe(&std::env::current_exe()?);
    registry::set_string(RUN_KEY, APP_NAME, &run_value(&exe.to_string_lossy()))?;
    // An explicit "enabled" flag, so that an old "disabled" flag left by Task Manager is overridden.
    registry::set_binary(APPROVED_KEY, APP_NAME, &STARTUP_ENABLED)
}
