//! Administrator mode: the per-user install, moved to run as administrator.
//!
//! Windows does not pass the input of an ordinary program to a window of a program that runs with
//! more rights (UIPI), so a CursorPie that runs from `%LOCALAPPDATA%` as the signed-in user does
//! nothing in Task Manager, in a terminal opened with *Run as administrator* or in an installer
//! that asked for UAC. Starting it elevated by hand works, but asks for UAC at every start and
//! cannot be done from the login entry. *Reinstall as administrator* (About tab) fixes that once:
//!
//! | Step | Who | What |
//! |---|---|---|
//! | 1 | the running copy | starts itself with `--install-admin` through UAC ([`reinstall_as_administrator`]), then quits |
//! | 2 | the elevated copy | waits for the first to quit, copies itself to `%ProgramFiles%\CursorPie`, makes the logon task (highest rights, this account only), writes the machine-wide "Apps & features" entry, starts the task ([`run_admin_install_if_asked`]) |
//! | 3 | the program the task started | removes what the per-user install left behind: the startup entry, the folder, the uninstall entry ([`settle`]) |
//!
//! **Why `%ProgramFiles%` and a task.** A startup entry in the registry cannot start a program
//! elevated without a UAC question, a task set to "run with highest privileges" can. What a task
//! runs elevated must not be writable by an ordinary program, or that program could replace it and
//! get administrator rights for free; `%LOCALAPPDATA%` is, `%ProgramFiles%` is not. So the program
//! is copied there, and the task file is written there too, where nobody else can change it between
//! writing it and handing it to `schtasks`. Nothing here is taken from the environment of the
//! caller: the program folder and the system folder are read from `HKEY_LOCAL_MACHINE`.
//!
//! **Afterwards**, a copy that is started without the rights (the Start menu, a download, a stale
//! startup entry) does not run: it starts the task, which runs the installed copy elevated, and
//! quits ([`settle`]). A newer copy offers to update the installed one first. *Launch at login* is
//! the flag of the logon trigger of the task, kept in `HKCU\Software\CursorPie` as well so reading it
//! needs no process ([`set_admin_login`]).
//!
//! Undo: *Apps & features* > CursorPie > Uninstall runs `cursorpie.exe --uninstall` of the installed
//! copy, which asks for UAC, removes the task, the entry and the folder, and leaves the settings.

use super::apps::own_process_elevated;
use super::install::{
    copy_over, install_dir_path, register_uninstall_in, same_file, uninstall_key,
};
use super::login_item::{launch_at_login_enabled, remove_user_entry, user_entry_exists};
use super::registry::{self, Hive};
use super::shell::run_shell;
use super::util::{
    AdminInstallRequest, EXE_NAME, SETUP_UNINSTALLER, UNINSTALL_ARG, account_name,
    admin_install_params, admin_installed_exe, cleanup_command, parse_admin_install, task_xml,
    utf16_bytes_to_string, utf16le_with_bom, wide,
};
use crate::about::{APP_NAME, VERSION};
use crate::platform::{AdminMode, ReinstallError};
use crate::update::Version;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{fs, io};
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_SETFOREGROUND, MB_YESNO,
    MessageBoxW, SW_HIDE,
};

/// Where the program keeps what the person chose, below `HKCU`.
const SETTINGS_KEY: &str = r"Software\CursorPie";

/// The value that remembers *Launch at login* in administrator mode (a `DWORD`, 0 or 1). A missing
/// value means yes, which is how the task is made.
const LOGIN_VALUE: &str = "AdminLaunchAtLogin";

/// How long the elevated step waits for the copy that asked for it to quit, in milliseconds.
const QUIT_WAIT_MS: u32 = 20_000;

/// What `ShellExecuteW` gives back when UAC was answered with No (`SE_ERR_ACCESSDENIED`), and the
/// last error that goes with it (`ERROR_CANCELLED`).
const SE_ERR_ACCESSDENIED: isize = 5;
const ERROR_CANCELLED: i32 = 1223;

// ---------------------------------------------------------------------------- where things are

/// A text value of the registry, `None` when it is missing or empty.
fn registry_text(hive: Hive, path: &str, name: &str) -> Option<String> {
    let text = utf16_bytes_to_string(&registry::get_bytes_in(hive, path, name)?);
    (!text.trim().is_empty()).then_some(text)
}

/// The program folder (`C:\Program Files`), as Windows records it. It is not read from an
/// environment variable: the elevated step must not put the program where the caller points.
fn program_files() -> String {
    registry_text(
        Hive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion",
        "ProgramFilesDir",
    )
    .unwrap_or_else(|| r"C:\Program Files".to_string())
}

/// The Windows folder (`C:\Windows`), as Windows records it.
fn windows_dir() -> String {
    registry_text(
        Hive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        "SystemRoot",
    )
    .unwrap_or_else(|| r"C:\Windows".to_string())
}

/// A program of `System32` by its full path, so that nothing the caller's `PATH` names can take
/// its place in a step that runs elevated.
fn system_tool(name: &str) -> PathBuf {
    PathBuf::from(format!(r"{}\System32\{name}", windows_dir()))
}

/// Where the program of the administrator install is.
fn admin_exe_path() -> PathBuf {
    PathBuf::from(admin_installed_exe(&program_files(), APP_NAME))
}

/// Whether CursorPie is installed in administrator mode: the machine-wide "Apps & features" entry
/// exists and the program it belongs to is still there.
pub(super) fn admin_install_present() -> bool {
    registry::get_bytes_in(Hive::LocalMachine, &uninstall_key(), "UninstallString").is_some()
        && admin_exe_path().is_file()
}

/// What the About tab shows (see [`AdminMode`]).
pub fn admin_mode() -> AdminMode {
    AdminMode {
        available: true,
        elevated: own_process_elevated(),
        installed: admin_install_present(),
    }
}

/// The account that runs this program, `DOMAIN\name`.
fn current_user_name() -> Option<String> {
    account_name(
        std::env::var("USERDOMAIN").ok().as_deref(),
        std::env::var("USERNAME").ok().as_deref(),
    )
}

// ---------------------------------------------------------------------------- small system calls

/// A message box without an owner. Returns the button that was pressed.
fn message_box(text: &str, style: u32) -> i32 {
    let (text, caption) = (wide(text), wide(APP_NAME));
    // SAFETY: both strings are zero terminated and outlive the call; a null owner is allowed.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), style) }
}

/// How a start with `runas` ended.
enum RunAs {
    Started,
    Cancelled,
    Failed,
}

/// Starts `exe` with `params` through UAC. The call returns when UAC was answered, not when the
/// program ends.
fn run_as(exe: &Path, params: &str) -> RunAs {
    let (verb, file, params) = (wide("runas"), wide(&exe.to_string_lossy()), wide(params));
    // SAFETY: every string is zero terminated and outlives the call; null is allowed for the owner
    // window and for the directory.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_HIDE,
        )
    } as isize;
    // read at once, before anything else can change it
    let last = io::Error::last_os_error().raw_os_error();
    if result > 32 {
        RunAs::Started
    } else if result == SE_ERR_ACCESSDENIED || last == Some(ERROR_CANCELLED) {
        RunAs::Cancelled
    } else {
        RunAs::Failed
    }
}

/// Waits until the process `pid` has ended, at most `millis`. A process that is gone, or that
/// cannot be opened, does not make it wait.
fn wait_for_exit(pid: u32, millis: u32) {
    // SAFETY: the handle is closed before returning; a null handle is not used.
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return;
        }
        WaitForSingleObject(process, millis);
        CloseHandle(process);
    }
}

/// Runs a program of `System32` with no window and no output. `Ok(true)` when it ended with 0.
fn run_system_tool(name: &str, args: &[&str]) -> io::Result<bool> {
    let mut cmd = Command::new(system_tool(name));
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    super::process::hide_console(&mut cmd);
    Ok(cmd.status()?.success())
}

/// Ends every other copy of the program, whoever started it: the elevated step cannot start the
/// installed copy while one holds the single-instance lock, nor replace a file that runs.
fn end_other_copies() {
    let image = format!("IMAGENAME eq {EXE_NAME}");
    let others = format!("PID ne {}", std::process::id());
    let _ = run_system_tool("taskkill.exe", &["/F", "/FI", &image, "/FI", &others]);
}

// ---------------------------------------------------------------------------- the logon task

/// Makes the logon task of `exe` for `user`, or makes it again (`/F`). The definition is written
/// next to the program, in the folder only administrators can write to, and removed again.
fn register_task(exe: &Path, user: &str, at_login: bool) -> io::Result<()> {
    let dir = exe
        .parent()
        .ok_or_else(|| io::Error::other("the program has no folder"))?;
    let file = dir.join("task.xml");
    let xml = task_xml(
        user,
        &exe.to_string_lossy(),
        &dir.to_string_lossy(),
        at_login,
    );
    fs::write(&file, utf16le_with_bom(&xml))?;
    let path = file.to_string_lossy().into_owned();
    let made = run_system_tool(
        "schtasks.exe",
        &["/Create", "/TN", APP_NAME, "/XML", &path, "/F"],
    );
    let _ = fs::remove_file(&file);
    match made {
        Ok(true) => Ok(()),
        Ok(false) => Err(io::Error::other(
            "Windows did not accept the task that starts CursorPie at login",
        )),
        Err(e) => Err(e),
    }
}

/// Removes the logon task. A task that is not there counts as removed.
fn remove_task() {
    let _ = run_system_tool("schtasks.exe", &["/Delete", "/TN", APP_NAME, "/F"]);
}

/// Starts the logon task: the installed copy runs elevated, in this session. This works without
/// administrator rights, the task belongs to the account that runs it. A task that already runs
/// is not started a second time (its policy), which is also a success.
fn start_task() -> bool {
    run_system_tool("schtasks.exe", &["/Run", "/TN", APP_NAME]).unwrap_or(false)
}

// ---------------------------------------------------------------------------- launch at login

/// *Launch at login* in administrator mode: what the person chose last (yes when nothing was).
pub(super) fn admin_login_enabled() -> bool {
    registry::get_bytes(SETTINGS_KEY, LOGIN_VALUE).is_none_or(|v| v.iter().any(|&b| b != 0))
}

/// Switches *Launch at login* of administrator mode: the logon trigger of the task, which only an
/// elevated copy may change. Nothing is done when it already is as asked, so Settings can call it
/// at every save.
pub(super) fn set_admin_login(on: bool) -> io::Result<()> {
    if admin_login_enabled() == on {
        return Ok(());
    }
    if !own_process_elevated() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "in administrator mode only a copy that runs as administrator can change Launch at login",
        ));
    }
    let user = current_user_name().ok_or_else(|| io::Error::other("the account is not known"))?;
    register_task(&admin_exe_path(), &user, on)?;
    registry::set_dword(SETTINGS_KEY, LOGIN_VALUE, u32::from(on))
}

// ---------------------------------------------------------------------------- reinstall

/// Starts the elevated step (the *Reinstall as administrator* button) and says whether Windows
/// let it start. The caller quits right after `Ok`: the elevated step waits for it, replaces the
/// files and starts the installed copy. Nothing has changed when this returns an error.
///
/// # Errors
///
/// [`ReinstallError::Cancelled`] when UAC was answered with No, [`ReinstallError::Failed`] when
/// the step could not be started.
pub fn reinstall_as_administrator() -> Result<(), ReinstallError> {
    let exe = std::env::current_exe().map_err(|_| ReinstallError::Failed)?;
    let user = current_user_name().ok_or(ReinstallError::Failed)?;
    let login = launch_at_login_enabled();
    let params = admin_install_params(&user, std::process::id(), login);
    match run_as(&exe, &params) {
        RunAs::Started => {
            // the choice goes along: the copy that starts as administrator reads it from here
            let _ = registry::set_dword(SETTINGS_KEY, LOGIN_VALUE, u32::from(login));
            Ok(())
        }
        RunAs::Cancelled => Err(ReinstallError::Cancelled),
        RunAs::Failed => Err(ReinstallError::Failed),
    }
}

/// The elevated step, when the command line asks for it (`--install-admin`). Returns `true` when
/// it did, and the caller then exits instead of starting the app. When the step fails, the person
/// is told and a copy is started again, so CursorPie never just disappears.
pub(super) fn run_admin_install_if_asked() -> bool {
    let Some(request) = parse_admin_install(std::env::args().skip(1)) else {
        return false;
    };
    if let Err(e) = install_as_administrator(&request) {
        eprintln!("CursorPie: the administrator install failed: {e}");
        let text = format!(
            "{APP_NAME} could not be installed as administrator.\n\n{e}\n\n\
             {APP_NAME} keeps running as it did before."
        );
        message_box(&text, MB_OK | MB_ICONERROR | MB_SETFOREGROUND);
        bring_back_a_copy();
    }
    true
}

/// The elevated step itself, see the table at the top.
fn install_as_administrator(request: &AdminInstallRequest) -> io::Result<()> {
    if !own_process_elevated() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "this step needs administrator rights",
        ));
    }
    let source = std::env::current_exe()?;
    let target = admin_exe_path();
    let user = request
        .user
        .clone()
        .or_else(current_user_name)
        .ok_or_else(|| io::Error::other("the account is not known"))?;

    // The copy that asked quits by itself; wait for it, then end what is left of any copy.
    if let Some(pid) = request.wait_pid {
        wait_for_exit(pid, QUIT_WAIT_MS);
    }
    end_other_copies();

    if !same_file(&source, &target) {
        copy_over(&source, &target)?;
    }
    register_task(&target, &user, request.launch_at_login)?;
    if let Err(e) = register_uninstall_in(Hive::LocalMachine, &target) {
        remove_task(); // no entry, no task: the install is all or nothing
        return Err(e);
    }
    // the task was only just made: give Windows a moment before the second try
    if !start_task() {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if !start_task() {
            return Err(io::Error::other(
                "the task that starts CursorPie could not be started",
            ));
        }
    }
    Ok(())
}

/// After a failed step: the installed copy when there is one and it starts, otherwise this copy,
/// started through Explorer so that it runs without the rights of this step.
fn bring_back_a_copy() {
    if admin_install_present() && start_task() {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(format!(r"{}\explorer.exe", windows_dir()))
            .arg(exe)
            .spawn();
    }
}

// ---------------------------------------------------------------------------- uninstall

/// The uninstall of the administrator install (what *Apps & features* runs for it), when this copy
/// is that install. Returns `true` when it was, whether or not the person went through with it.
///
/// *Apps & features* does not ask for UAC on its own, so a copy without the rights asks here and
/// leaves the work to the elevated copy it starts.
pub(super) fn uninstall_if_administrator_install() -> bool {
    let Ok(current) = std::env::current_exe() else {
        return false;
    };
    if !admin_install_present() || !same_file(&current, &admin_exe_path()) {
        return false;
    }
    if own_process_elevated() {
        remove_admin_install(&current);
    } else if !matches!(
        run_as(&current, UNINSTALL_ARG),
        RunAs::Started | RunAs::Cancelled
    ) {
        message_box(
            &format!("{APP_NAME} could not ask for administrator rights, so it was not removed."),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
    true
}

/// Removes the task, the machine-wide entry, the browser link and the folder. The settings stay.
fn remove_admin_install(exe: &Path) {
    remove_task();
    let _ = registry::delete_key_in(Hive::LocalMachine, &uninstall_key());
    let _ = registry::delete_value(SETTINGS_KEY, LOGIN_VALUE);
    crate::bridge::install::uninstall(); // the browser extension's host entries
    let text = format!(
        "{APP_NAME} was removed. Your settings were kept in {}.",
        crate::config::config_path().display()
    );
    message_box(&text, MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND);
    // This copy may be the running one: the folder goes after it has quit, and every other copy
    // (an elevated one included, this step has the rights) is ended first.
    if let Some(dir) = exe.parent() {
        run_shell(&cleanup_command(&dir.to_string_lossy(), std::process::id()));
    }
}

// ---------------------------------------------------------------------------- every start

/// What a start does when CursorPie is installed in administrator mode (the first thing the
/// install step asks, see `install::ensure_installed`). Returns `true` when there is nothing more
/// for the install step to do: this copy is the installed one, or an elevated copy that the person
/// started on purpose, or it could not hand over. Returns `false` when there is no administrator
/// install at all.
///
/// A copy that runs without the rights does not run: it asks for the installed copy to be started
/// (the task) and quits, so a click on an old shortcut or a stale startup entry cannot bring the
/// limits back. A copy that is newer than the installed one offers to update it first.
pub(super) fn settle(current: &Path) -> bool {
    if !admin_install_present() {
        return false;
    }
    let installed = admin_exe_path();
    if same_file(current, &installed) {
        clean_user_scope();
        return true;
    }
    if own_process_elevated() {
        return true;
    }
    if newer_than_installed() && asks_to_update() && reinstall_as_administrator().is_ok() {
        std::process::exit(0); // the elevated step waits for this copy to be gone
    }
    if start_task() {
        std::process::exit(0);
    }
    true
}

/// Whether this program is a newer version than the one installed in administrator mode.
fn newer_than_installed() -> bool {
    let installed = registry_text(Hive::LocalMachine, &uninstall_key(), "DisplayVersion")
        .and_then(|v| Version::parse(&v));
    matches!((Version::current(), installed), (Some(me), Some(there)) if me > there)
}

/// Asks, once per start, whether this newer copy may update the installed one.
fn asks_to_update() -> bool {
    let text = format!(
        "{APP_NAME} {VERSION} was started, and an older version is installed as administrator.\n\n\
         Update the installed one with this copy? Windows asks for your permission, and your \
         settings are kept.\n\n\
         Choose No to start the installed version."
    );
    message_box(&text, MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND) == IDYES
}

/// Removes what the per-user install left behind, now that the administrator install runs: the
/// startup entry (it would start a copy without the rights at every login), and for a copy that
/// installed itself from the zip its folder and its "Apps & features" entry. A copy that the Setup
/// wizard installed keeps its folder, its Start menu shortcut and its own entry: it is what the
/// wizard owns, and a copy started from it hands over to the installed one anyway.
///
/// Checks first and writes only when something is there, so a normal start changes nothing.
fn clean_user_scope() {
    if user_entry_exists() {
        let _ = remove_user_entry();
    }
    let Some(dir) = install_dir_path().filter(|d| d.is_dir()) else {
        return;
    };
    if dir.join(SETUP_UNINSTALLER).is_file() {
        return;
    }
    if fs::remove_dir_all(&dir).is_err() {
        return; // a file is still in use: the next start tries again
    }
    let _ = registry::delete_key(&uninstall_key());
}
