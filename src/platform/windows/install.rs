//! Per-user install: makes CursorPie a real installed program instead of a loose `.exe`.
//!
//! The first time a packaged build starts, [`ensure_installed`] copies the executable to
//! `%LOCALAPPDATA%\Programs\CursorPie`, turns on "Launch at login" pointing at that copy, and adds
//! an "Apps & features" entry whose uninstall command is `cursorpie.exe --uninstall`
//! ([`handle_command_line`]). Nothing needs administrator rights.
//!
//! The first install asks for permission (once; a "No" is remembered under
//! `HKCU\Software\CursorPie\InstallDeclined`, delete that value to be asked again).
//!
//! Starting from a download folder or an unzipped archive therefore stops mattering: the copy in
//! the install folder is what starts at every login, and starting a newer download updates it.
//! Builds made by `cargo` are left alone, so development never touches the installed app (set
//! `CURSORPIE_NO_INSTALL=1` to get the same for any other copy). A copy that the Setup wizard
//! (`installer/cursorpie.iss`) put in place is left alone too: the wizard registered it.

use super::login_item::{launch_at_login_enabled, repair_launch_entry, set_launch_at_login};
use super::registry;
use super::shell::run_shell;
use super::util::{
    cleanup_command, install_dir, installed_by_setup, installed_exe, is_build_output, same_path,
    uninstall_command, wants_uninstall,
};
use crate::about::{APP_NAME, AUTHOR_NAME, HANDOVER_ARG, REPO_URL, VERSION};
use std::path::{Path, PathBuf};
use std::{fs, io};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO, MessageBoxW,
};

/// Environment variable that switches the install step off.
const OPT_OUT: &str = "CURSORPIE_NO_INSTALL";

/// Where the program remembers that the user said "No" to the install question.
const SETTINGS_KEY: &str = r"Software\CursorPie";
const DECLINED: &str = "InstallDeclined";

/// Asks, once, whether this copy may install itself. A program that silently copies itself into
/// the profile, registers a startup entry and starts the copy is exactly what a dropper does, and
/// behavior-based antivirus engines judge it that way; asking first makes it the user's decision.
fn user_agrees_to_install(dir: &Path) -> bool {
    let text = format!(
        "Install {APP_NAME} for this user?\n\n\
         It will be copied to {}, listed in Apps & features and started at every login. \
         Nothing needs administrator rights.\n\n\
         Choose No to just run it from where it is.",
        dir.display()
    );
    let (text, caption) = (super::util::wide(&text), super::util::wide(APP_NAME));
    // SAFETY: both strings are zero terminated and outlive the call; a null owner is allowed.
    let answer = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_YESNO | MB_ICONQUESTION,
        )
    };
    answer == IDYES
}

/// The "Apps & features" entry of this app.
fn uninstall_key() -> String {
    format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{APP_NAME}")
}

/// `%LOCALAPPDATA%`, if the user session has one.
fn local_app_data() -> Option<String> {
    std::env::var("LOCALAPPDATA")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Whether two paths are the same file. The text comparison ([`same_path`]) is tried first; when
/// it says no, both paths are resolved by the file system, which turns 8.3 short names (`JOHNDO~1`),
/// junctions and links into the one real path. Without that, an installed copy that was started
/// through such a path took itself for another copy.
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    same_path(&a.to_string_lossy(), &b.to_string_lossy())
        || matches!(
            (fs::canonicalize(a), fs::canonicalize(b)),
            (Ok(x), Ok(y)) if same_path(&x.to_string_lossy(), &y.to_string_lossy())
        )
}

/// The install folder.
fn install_dir_path() -> Option<PathBuf> {
    local_app_data().map(|l| PathBuf::from(install_dir(&l, APP_NAME)))
}

/// Where the installed executable lives.
fn installed_exe_path() -> Option<PathBuf> {
    local_app_data().map(|l| PathBuf::from(installed_exe(&l, APP_NAME)))
}

/// The executable the startup entry should start: the installed copy when there is one, otherwise
/// `current` (a build that is not installed, such as one made by `cargo`).
pub(super) fn startup_exe(current: &Path) -> PathBuf {
    installed_exe_path()
        .filter(|p| p.is_file())
        .unwrap_or_else(|| current.to_path_buf())
}

/// Whether an earlier run completed the install (the entry exists and the file is still there).
fn is_installed() -> bool {
    registry::has_value(&uninstall_key(), "UninstallString")
        && installed_exe_path().is_some_and(|p| p.is_file())
}

/// Whether both files hold the same bytes.
fn same_contents(a: &Path, b: &Path) -> bool {
    matches!((fs::read(a), fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// Puts `src` at `dst`. A running program cannot be overwritten, but it can be renamed, so the old
/// copy is moved aside first; that is what lets a newer download update an app that is running.
fn copy_over(src: &Path, dst: &Path) -> io::Result<()> {
    if let Some(dir) = dst.parent() {
        fs::create_dir_all(dir)?;
    }
    if same_contents(src, dst) {
        return Ok(());
    }
    let old = leftover(dst);
    let _ = fs::remove_file(&old);
    let moved = dst.exists();
    if moved {
        fs::rename(dst, &old)?;
    }
    if let Err(e) = fs::copy(src, dst) {
        if moved {
            let _ = fs::rename(&old, dst); // keep the working copy
        }
        return Err(e);
    }
    let _ = fs::remove_file(&old); // fails while the old copy still runs; the next start retries
    Ok(())
}

/// The name an outdated copy is parked under while it is replaced.
fn leftover(exe: &Path) -> PathBuf {
    exe.with_extension("exe.old")
}

/// Writes the "Apps & features" entry.
fn register_uninstall(exe: &Path) -> io::Result<()> {
    let key = uninstall_key();
    let dir = exe.parent().unwrap_or(exe).to_string_lossy().into_owned();
    let exe_str = exe.to_string_lossy();
    for (name, value) in [
        ("DisplayName", APP_NAME),
        ("DisplayVersion", VERSION),
        ("Publisher", AUTHOR_NAME),
        ("URLInfoAbout", REPO_URL),
        ("InstallLocation", dir.as_str()),
        ("DisplayIcon", &exe_str),
        ("UninstallString", &uninstall_command(&exe_str)),
    ] {
        registry::set_string(&key, name, value)?;
    }
    // no Modify or Repair buttons: there is nothing to modify or repair
    registry::set_dword(&key, "NoModify", 1)?;
    registry::set_dword(&key, "NoRepair", 1)
}

/// Installs this program for the current user and keeps the installation current.
///
/// Returns the path of the installed copy when the caller is a *different* copy that should hand
/// over: it starts the returned program and exits. `None` means "carry on as you are": already
/// running from the install folder, a `cargo` build, an opt-out, or an install that failed (the
/// app still works, it just will not come back after a restart; the reason is printed).
///
/// The first install also turns "Launch at login" on. Later runs only repair the startup entry
/// when it is on, so a user who switched it off keeps it off.
pub fn ensure_installed() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
    if std::env::var_os(OPT_OUT).is_some() || is_build_output(&current.to_string_lossy()) {
        return None;
    }
    // The Setup wizard installed this copy and owns its uninstall entry and startup entry.
    if installed_by_setup(&current) {
        return None;
    }
    // Handed over to by another copy: this is the installed copy, whatever the paths look like.
    // Never hand over again, or a path that is written differently starts copies of itself forever.
    let handed_over = std::env::args().skip(1).any(|a| a == HANDOVER_ARG);
    let handover = if handed_over {
        None
    } else {
        install_step(&current)
    };
    if handover.is_none() {
        // Staying in this copy: make sure a login entry that cannot work is repaired.
        repair_launch_entry();
    }
    handover
}

/// The install itself, see [`ensure_installed`]. Returns the installed copy when this copy has to
/// hand over to it.
fn install_step(current: &Path) -> Option<PathBuf> {
    let target = installed_exe_path()?;
    // A Setup wizard install of the same folder is left alone: it has its own uninstall entry, and
    // copying over it would leave two of them.
    if installed_by_setup(&target) {
        return None;
    }
    let running_installed = same_file(current, &target);
    let first_time = !is_installed();

    // A first install is the user's decision. An update of an app that is already installed
    // needs no question: the user agreed when they installed it.
    if first_time && !running_installed {
        if registry::has_value(SETTINGS_KEY, DECLINED) {
            return None;
        }
        let dir = target.parent().unwrap_or(&target);
        if !user_agrees_to_install(dir) {
            let _ = registry::set_dword(SETTINGS_KEY, DECLINED, 1);
            return None;
        }
    }

    if running_installed {
        let _ = fs::remove_file(leftover(&target)); // an update parked the old copy here
    } else if let Err(e) = copy_over(current, &target) {
        eprintln!("CursorPie: could not install to {}: {e}", target.display());
        return None;
    }
    // Registry writes happen only when something was installed or updated, not at every start:
    // a program that rewrites its startup entry each time it runs looks like malware to
    // behavior-based antivirus engines.
    let changed = first_time || !running_installed;
    if changed && let Err(e) = register_uninstall(&target) {
        eprintln!("CursorPie: could not write the uninstall entry: {e}");
    }
    if changed
        && (first_time || launch_at_login_enabled())
        && let Err(e) = set_launch_at_login(true)
    {
        eprintln!("CursorPie: could not set up launch at login: {e}");
    }
    (!running_installed).then_some(target)
}

/// Handles a command-line switch. Returns `true` when the program should exit now instead of
/// starting the app (today only `--uninstall`).
pub fn handle_command_line() -> bool {
    if !wants_uninstall(std::env::args().skip(1)) {
        return false;
    }
    uninstall();
    true
}

/// Removes the startup entry, the "Apps & features" entry and the install folder. The settings in
/// `config.json` stay, so a later install picks them up again.
fn uninstall() {
    let _ = set_launch_at_login(false);
    crate::bridge::install::uninstall(); // the browser extension's host entries
    let _ = registry::delete_key(&uninstall_key());
    // Asked first: the cleanup below runs a few seconds after it is started, and by then this
    // process (which may be the installed copy) must be gone so the file can be deleted.
    let text = format!(
        "{APP_NAME} was removed. Your settings were kept in {}.",
        crate::config::config_path().display()
    );
    let (text, caption) = (super::util::wide(&text), super::util::wide(APP_NAME));
    // SAFETY: both strings are zero terminated and outlive the call; a null owner is allowed.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
    if let Some(dir) = install_dir_path() {
        run_shell(&cleanup_command(&dir.to_string_lossy(), std::process::id()));
    }
}
