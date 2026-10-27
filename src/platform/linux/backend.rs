//! The functions every platform backend provides (see the table in `platform/mod.rs`), for Linux.
//!
//! This first release supports X11 sessions (and XWayland for the windows it can see). What it does
//! not do yet, and why, is listed in `docs/LINUX_MIGRATION.md`: app icons, a tray icon and
//! keyboard-layout labels fall back to the same behavior as the stub, and Wayland-native windows are
//! invisible to it.

use super::desktop_entry::Index;
use super::launch::{self, Chooser, Launch};
use super::{autostart, x11, xdg, xlib};
use crate::config::DesktopMode;
use crate::geometry::Point;
use crate::keys::Shortcut;
use crate::platform::{FileKind, NativeIcon, ScreenRect, spawn_reaped};
use crate::tap::Kind;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// ------------------------------------------------------------------------------------ apps

/// The app in front.
pub fn frontmost_bundle_id() -> Option<String> {
    x11::conn()?.frontmost()
}

/// The app of the window under the logical point `p`.
pub fn bundle_id_at(p: Point) -> Option<String> {
    x11::conn()?.app_at(p)
}

/// The app id of a picked file (a `.desktop` file or an executable).
pub fn bundle_id_of(app_path: &str) -> Option<String> {
    launch::app_id_of_path(app_path)
}

/// Icons are not read yet; the pie shows its own glyph instead.
pub fn app_icon(_bundle: &str) -> Option<NativeIcon> {
    None
}

/// There is no Dock to hide the app from.
pub fn set_accessory_policy() {}

/// The window manager draws no frame around an undecorated window.
pub fn clear_window_frame(_handle: isize) {}

/// X11 window managers (KWin, ...) may place a window that is mapped again at the center of the
/// screen, ignoring the position it was given while hidden, so the pie asks for it again.
pub fn window_manager_may_recenter() -> bool {
    true
}

/// The pointer position in logical points.
pub fn cursor_pos() -> Option<(f64, f64)> {
    x11::conn()?.pointer()
}

/// The monitors in logical points.
pub fn display_rects() -> Vec<ScreenRect> {
    x11::conn().map(|c| c.monitors()).unwrap_or_default()
}

// ------------------------------------------------------------------------------ permission

/// X11 asks for no permission.
pub fn is_trusted() -> bool {
    true
}
/// X11 asks for no permission.
pub fn is_trusted_cached() -> bool {
    true
}
/// Nothing to open.
pub fn open_accessibility_settings() {}
/// Nothing to open.
pub fn open_input_monitoring_settings() {}

// ------------------------------------------------------------------------------- injection

/// Sends a shortcut to the window that has the keyboard focus.
pub fn press_shortcut(sc: Shortcut) {
    if let Some(c) = x11::conn() {
        c.press_shortcut(sc);
    }
}

/// The X button number of a macOS button number (0 left, 1 right, 2 middle, 3 back, 4 forward).
pub(crate) fn x_button_for(button: i64) -> Option<i32> {
    match button {
        0 => Some(1),
        1 => Some(3),
        2 => Some(2),
        3 => Some(8),
        4 => Some(9),
        _ => None,
    }
}

/// Re-posts a swallowed press or release of `button`. The event goes to the current pointer
/// position, so `_pos` is not needed.
pub fn post_mouse(_pos: Point, kind: Kind, button: i64) {
    let Some(xb) = x_button_for(button) else {
        return;
    };
    let press = match kind {
        Kind::Down => true,
        Kind::Up => false,
        Kind::Drag => return,
    };
    if let Some(c) = x11::conn() {
        c.inject_button(xb, press);
    }
}

/// Brings the top-most window of `bundle` to the front.
pub fn activate_app(bundle: &str) {
    if let Some(c) = x11::conn() {
        c.activate(bundle);
    }
}

// --------------------------------------------------------------------------- launching things

/// Whether `name` is an executable file somewhere on `PATH`.
fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        std::fs::metadata(dir.join(name))
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

/// A command with no input and no output, in its own process group so a signal sent to CursorPie
/// does not reach it.
fn detached(program: &str) -> Command {
    let mut c = Command::new(program);
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    c
}

/// Starts `cmd` and reports whether it could be started.
fn started(cmd: &mut Command) -> bool {
    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// Opens an app: a desktop id, a `.desktop` file, a program on `PATH` or a path.
pub fn open_app(app: &str) {
    let a = app.trim();
    if a.is_empty() {
        return;
    }
    let index = x11::index().unwrap_or_else(|| std::sync::Arc::new(Index::default()));
    let how = if a.ends_with(".desktop") && a.contains('/') {
        Launch::Desktop(a.to_string())
    } else {
        launch::resolve(a, &index)
    };
    match how {
        Launch::Desktop(path) => {
            let mut gio = detached("gio");
            gio.arg("launch").arg(&path);
            if !started(&mut gio) {
                spawn_reaped(detached("xdg-open").arg(&path));
            }
        }
        Launch::Program(p) => spawn_reaped(&mut detached(&p)),
    }
}

/// Opens an app, going to its workspace when it already runs on another one. The window manager
/// does the switching when it is asked to activate a window, so this raises the app's window if it
/// has one and starts the app otherwise. `mode` is `SwitchToApp` here: `DoNothing` never gets this far
/// and `BringToCurrent` is turned into `SwitchToApp` for Linux (`DesktopMode::effective`).
pub fn reveal_app(app: &str, _mode: DesktopMode) {
    let a = app.trim();
    if a.is_empty() {
        return;
    }
    if let Some(c) = x11::conn()
        && c.raise(a)
    {
        return;
    }
    open_app(a);
}

/// Opens a link or a file with the default application.
pub fn open_url(target: &str) {
    spawn_reaped(detached("xdg-open").arg(target));
}

/// Runs a shell command.
pub fn run_shell(cmd: &str) {
    spawn_reaped(detached("/bin/sh").arg("-c").arg(cmd));
}

/// Opens a file or folder with the default application.
pub fn open_path(path: &Path) {
    spawn_reaped(detached("xdg-open").arg(path));
}

/// Shows `path` in the file manager: a folder is opened, a file's folder is.
pub fn reveal_in_file_manager(path: &Path) {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"))
    };
    spawn_reaped(detached("xdg-open").arg(dir));
}

/// Asks the user for a file with `zenity` or `kdialog`. `None` if they cancel or neither exists.
pub fn choose_file(kind: FileKind, prompt: &str) -> Option<String> {
    let tool = if on_path("zenity") {
        Chooser::Zenity
    } else if on_path("kdialog") {
        Chooser::Kdialog
    } else {
        eprintln!("CursorPie: install zenity or kdialog to choose files, or type the path");
        return None;
    };
    let out = Command::new(tool.program())
        .args(launch::chooser_args(tool, kind, prompt))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8(out.stdout).ok()?;
    let path = path.trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// A soft hover sound through the sound theme, if `canberra-gtk-play` is installed. At most about
/// fifteen times a second.
pub fn tick() {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    if !*AVAILABLE.get_or_init(|| on_path("canberra-gtk-play")) {
        return;
    }
    {
        let mut last = crate::config::lock(&LAST);
        if last.is_some_and(|t| t.elapsed() < Duration::from_millis(66)) {
            return;
        }
        *last = Some(Instant::now());
    }
    spawn_reaped(detached("canberra-gtk-play").args(["-i", "button-pressed"]));
}

// ------------------------------------------------------------------------ session lifecycle

fn autostart_path() -> PathBuf {
    let home = crate::config::home();
    xdg::config_home(&home, std::env::var("XDG_CONFIG_HOME").ok().as_deref())
        .join("autostart")
        .join(autostart::FILE_NAME)
}

/// Whether the autostart entry exists and no desktop has switched it off.
pub fn launch_at_login_enabled() -> bool {
    std::fs::read_to_string(autostart_path())
        .map(|t| autostart::is_enabled(&t))
        .unwrap_or(false)
}

/// Writes or removes the autostart entry. The entry starts the program that is running now: the
/// AppImage file itself for an AppImage, not its temporary mount.
pub fn set_launch_at_login(on: bool) -> std::io::Result<()> {
    let path = autostart_path();
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let exec = match std::env::var("APPIMAGE") {
        Ok(p) if !p.trim().is_empty() => p,
        _ => std::env::current_exe()?.to_string_lossy().into_owned(),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, autostart::contents(&exec))
}

/// Distributions install the program; there is no self-install.
pub fn ensure_installed() -> Option<PathBuf> {
    None
}

/// No command-line switches yet.
pub fn handle_command_line() -> bool {
    false
}

/// Takes a lock file in the runtime directory. `false` if another copy holds it.
pub fn single_instance() -> bool {
    static HELD: OnceLock<std::fs::File> = OnceLock::new();
    let path = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if d.starts_with('/') => PathBuf::from(d).join("cursorpie.lock"),
        // SAFETY: `getuid` has no arguments and cannot fail.
        _ => PathBuf::from(format!("/tmp/cursorpie-{}.lock", unsafe { xlib::getuid() })),
    };
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
    else {
        return true; // cannot tell: do not stop the user from starting
    };
    use std::os::fd::AsRawFd;
    // SAFETY: the descriptor belongs to `file`, which is kept alive below.
    let locked = unsafe { xlib::flock(file.as_raw_fd(), xlib::LOCK_EX_NB) } == 0;
    if locked {
        let _ = HELD.set(file);
    }
    locked
}

/// Gives freed heap pages back to the OS (glibc only). The amount is not measured.
pub fn release_unused_memory() -> usize {
    #[cfg(target_env = "gnu")]
    // SAFETY: `malloc_trim` is always safe to call.
    unsafe {
        xlib::malloc_trim(0);
    }
    0
}

/// Keyboard layout: there is no snapshot yet, so labels use the US/ANSI fallback table.
pub mod layout {
    /// No snapshot to build.
    pub fn init() {}
    /// No snapshot to build.
    pub fn refresh() {}
    /// Always `None`.
    pub fn char_for(_code: u16, _shift: bool) -> Option<String> {
        None
    }
    /// Always `None`.
    pub fn find_key(_text: &str) -> Option<(u16, u8)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_buttons_map_to_x_buttons() {
        assert_eq!(x_button_for(0), Some(1));
        assert_eq!(x_button_for(1), Some(3), "right is X button 3");
        assert_eq!(x_button_for(2), Some(2), "middle is X button 2");
        assert_eq!(x_button_for(3), Some(8), "back");
        assert_eq!(x_button_for(4), Some(9), "forward");
        assert_eq!(x_button_for(5), None);
        assert_eq!(x_button_for(-1), None);
    }

    #[test]
    fn autostart_lives_in_the_config_directory() {
        let p = autostart_path();
        assert!(p.ends_with("autostart/cursorpie.desktop"), "{p:?}");
    }

    #[test]
    fn programs_on_path_are_found() {
        assert!(on_path("sh"));
        assert!(!on_path("definitely-not-a-program-cursorpie"));
    }
}
