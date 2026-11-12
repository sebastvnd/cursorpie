//! Operating-system glue: permissions, running apps, launch-at-login, file choosers, input injection.
//!
//! Exactly one **backend** is compiled in and re-exported here, so the rest of the crate calls
//! `platform::frontmost_bundle_id()` and never contains a `cfg` attribute:
//!
//! | Backend | Selected on | Notes |
//! |---|---|---|
//! | `macos` | `target_os = "macos"` | Core Graphics, AppKit, TIS, LaunchAgent |
//! | `windows` | `windows` | Win32 through `windows-sys` |
//! | `linux` | `target_os = "linux"` | X11 through Xlib, XInput2, XTest and XRandR (see `docs/LINUX_MIGRATION.md`) |
//! | `stub` | anything else | does nothing; only exists so `cargo test` runs on other Unixes |
//!
//! Every backend provides the same functions (the list is in the table below); a missing or
//! differently-typed function is a compile error on that target. This module holds what is shared:
//! the [`Os`] description (names, default modifier, hints), plain data types and the pure helpers
//! that decide things (app id matching), so they are unit tested on every OS.
//!
//! | Function | Meaning |
//! |---|---|
//! | `frontmost_bundle_id`, `bundle_id_at`, `bundle_id_of` | app id of the app in front, under a point, or of an executable |
//! | `app_icon` | the real icon of an app, see [`NativeIcon`] |
//! | `set_accessory_policy` | hide the Dock icon (macOS only, no-op elsewhere) |
//! | `clear_window_frame` | strip the OS frame of the native window handle (Windows only, no-op elsewhere) |
//! | `set_window_passive` | the window that shows the notice of an action must not take the focus (Windows: `WS_EX_NOACTIVATE`; no-op elsewhere) |
//! | `window_manager_may_recenter` | whether the window manager may ignore the position of a hidden window when it is shown again (X11 only) |
//! | `window_point` | where to ask the toolkit to put a window centered on a point of the pointer's space (identity except on Linux in a Wayland session, where KWin's points and XWayland's differ) |
//! | `window_scale` | how many of the toolkit's points one point of the pointer's space is, to turn a distance of the pointer into a distance in the pie (1 except on Linux in a Wayland session, where KWin's points and XWayland's differ) |
//! | `raise_own_window` | bring CursorPie's own window (Settings) to the front, on whichever workspace it is (X11: the toolkit's own request is refused there; Windows: the toolkit's request injects an Alt key press that eats the next click, so this is the only request sent; macOS: the toolkit's is enough) |
//! | `cursor_pos`, `display_rects` | pointer position and display bounds, in logical points |
//! | `is_trusted_cached`, `open_accessibility_settings`, `open_input_monitoring_settings` | input permission |
//! | `mouse_devices` | the mice the OS lists and their extra buttons, see [`mouse`](crate::mouse) |
//! | `press_shortcut`, `post_mouse`, `activate_app` | input injection (used by [`actions`](crate::actions)) |
//! | `open_app`, `reveal_app`, `open_url`, `run_shell`, `open_path`, `reveal_in_file_manager` | launching things; `reveal_app` is `open_app` that also goes to, or fetches, the app's desktop (see [`desktops`]) |
//! | `window_ids` | the windows of an app told apart by a number that stays the same for as long as a window lives (Windows: window handles, hidden ones included; X11 and KWin: the window manager's ids; macOS and the stub: `None`, the count is used); with it a window that was hidden and is shown again is not taken for a new one |
//! | `window_count` | how many windows an app has (Windows: its top-level windows, X11 and KWin: the window list, macOS: the layer-0 windows of its process); compared before and after a new window was asked for, it teaches [`learn`](crate::learn) which apps cannot open another one |
//! | `declares_single_window` | whether an app says it has a single main window, so the "Open app" choice leaves out the new-window badge (Linux: `SingleMainWindow` in the `.desktop` file; elsewhere false, and [`windowing`](crate::windowing) has a list of well-known one-window apps that applies on every OS) |
//! | `running_among`, `open_app_new_window` | for the choice an "Open app" slice offers when its app already runs (see [`running`](crate::running)): which of some apps run (one look at the system for all of them, called off the tap thread), and a new window of an app (macOS: `open -a`, then Cmd+N; Windows and Linux: start it again) |
//! | [`web`] | the same choice for an "Open URL" slice with a web link: which pages CursorPie opened and are still open, a new browser window, and going to the browser that has the page (not a backend function: it is built from `open_url`, `reveal_app` and `running_among`) |
//! | [`native`] | what the blocks of a flow ask of the desktop beyond the backend: the clipboard, notifications, questions in a window, a shell command that is waited for, text files (not a backend function: it runs the programs each system ships) |
//! | [`http`] | a plain HTTPS `GET` for the update check (see [`update`](crate::update)): not a backend function, it is the same code on every OS and runs the system's `curl` (or `wget`) |
//! | `choose_file`, `tick` | file chooser and hover sound |
//! | `haptic` | one haptic tick on a trackpad (macOS only, no-op elsewhere), see [`HapticKind`] |
//! | `launch_at_login_enabled`, `set_launch_at_login` | start with the user session |
//! | `ensure_installed`, `handle_command_line`, `single_instance` | Windows install step, `--uninstall`, one running copy (no-ops elsewhere) |
//! | `wait_for_login_ready` | after a login start (`--autostart`) wait for the desktop (Windows only, no-op elsewhere) |
//! | `release_unused_memory` | give freed pages back to the OS, without evicting what the next press needs |
//! | `stay_responsive` | keep the OS from throttling the hidden program (App Nap on macOS, power throttling on Windows; no-op on Linux) |
//! | [`layout`] | keyboard-layout snapshot |
//!
//! The **app id** is the key of per-app profiles: the bundle identifier on macOS
//! (`com.apple.Safari`), the lower-case executable name on Windows (`chrome.exe`) and the lower-case
//! desktop-entry id on Linux (`firefox`, `org.mozilla.firefox`).

pub mod desktops;
// The one HTTPS request of the update check, made with the system's `curl` (or `wget`): the same
// code on every OS, so no backend has to provide it.
pub mod http;
pub mod installed;
pub mod keymap;
// Clipboard, notifications, questions and a command that is waited for, for the blocks of a flow:
// built from the programs of each system, so it is the same on every OS.
pub mod native;
// Which web pages are open, a new browser window, going to the page that is open: built from the
// functions of the backends, so it is the same on every OS.
pub mod web;

// Runs the file choosers that are separate programs: zenity and kdialog on Linux, osascript on macOS.
#[cfg(unix)]
pub(crate) mod chooser;

// The pure Linux helpers (session, desktop entries, window hit test, XDG paths) compile on every OS so
// their tests run everywhere; the X11 backend inside is only compiled on Linux.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) mod linux;
#[cfg(target_os = "linux")]
pub use linux::backend::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

// Compiled on Windows, and on request elsewhere so it can be type-checked and its pure helpers
// tested (`--features check-windows`); only re-exported on Windows.
#[cfg(any(windows, feature = "check-windows"))]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod stub;
#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
pub use stub::*;

use crate::config::DesktopMode;
use crate::keys::{MOD_CMD, MOD_CTRL};
use std::process::Command;

/// A supported operating system. Defaults, labels and paths take an `Os` as a parameter so tests can
/// exercise both without running on both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

/// How the four modifier bits of a shortcut are named on an OS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModNames {
    pub ctrl: &'static str,
    pub alt: &'static str,
    pub shift: &'static str,
    pub cmd: &'static str,
}

impl Os {
    /// The OS this build runs on. Targets with no backend (the stub) report `Mac`, so the macOS
    /// characterization tests run unchanged there; tests that must hold on every build target pin
    /// their OS with the `_for(Os::Mac)` forms instead.
    pub const fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            Os::Mac
        }
    }

    /// The modifier that built-in shortcuts use for "the command key": Cmd on macOS, Ctrl elsewhere.
    pub const fn primary_mod(self) -> u8 {
        match self {
            Os::Mac => MOD_CMD,
            Os::Windows | Os::Linux => MOD_CTRL,
        }
    }

    /// Names of the modifiers in shortcut labels.
    pub const fn mod_names(self) -> ModNames {
        match self {
            Os::Mac => ModNames {
                ctrl: "Ctrl",
                alt: "Opt",
                shift: "Shift",
                cmd: "Cmd",
            },
            Os::Windows => ModNames {
                ctrl: "Ctrl",
                alt: "Alt",
                shift: "Shift",
                cmd: "Win",
            },
            Os::Linux => ModNames {
                ctrl: "Ctrl",
                alt: "Alt",
                shift: "Shift",
                cmd: "Super",
            },
        }
    }

    /// Name of the key combined with Z, C, V... by the Settings window itself (egui's "command").
    pub const fn primary_name(self) -> &'static str {
        match self {
            Os::Mac => "Cmd",
            Os::Windows | Os::Linux => "Ctrl",
        }
    }

    /// What an "Open app" slice can do when the app's window is on another desktop, in the order
    /// Settings lists them (see [`DesktopMode`]).
    ///
    /// Every OS can switch to the app's desktop. Only macOS can try to pull the window onto the
    /// current Space, through private system calls: the public API of Windows cannot move the
    /// window of another process, and neither can an X11 client in general.
    pub const fn desktop_modes(self) -> &'static [DesktopMode] {
        match self {
            Os::Mac => &[
                DesktopMode::SwitchToApp,
                DesktopMode::BringToCurrent,
                DesktopMode::DoNothing,
            ],
            Os::Windows | Os::Linux => &[DesktopMode::SwitchToApp, DesktopMode::DoNothing],
        }
    }

    /// Whether the event tap needs a permission the user must grant (macOS Accessibility).
    pub const fn needs_permission(self) -> bool {
        matches!(self, Os::Mac)
    }

    /// Mouse buttons that can be chosen as the trigger (macOS numbering: 2 = middle, 5.. = extra).
    pub const fn trigger_buttons(self) -> &'static [i64] {
        match self {
            Os::Mac => &[2, 5, 6, 7],
            // Linux: extra buttons have no stable numbers yet (docs/LINUX_MIGRATION.md, Appendix B)
            Os::Windows | Os::Linux => &[2],
        }
    }

    /// The file manager, for button labels.
    pub const fn file_manager(self) -> &'static str {
        match self {
            Os::Mac => "Finder",
            Os::Windows => "Explorer",
            Os::Linux => "file manager",
        }
    }

    /// Where the tray icon lives, for labels and messages.
    pub const fn tray_name(self) -> &'static str {
        match self {
            Os::Mac => "menu bar",
            Os::Windows => "system tray",
            Os::Linux => "system tray",
        }
    }

    /// Whether this OS can open the pie with a hold on a trackpad (the Trackpad panel of Settings is
    /// shown only where it can): a Mac trackpad, a Windows precision touchpad, or a Linux (X11)
    /// touchpad.
    pub const fn has_trackpad(self) -> bool {
        matches!(self, Os::Mac | Os::Windows | Os::Linux)
    }

    /// Whether this OS can give haptic feedback on a trackpad (the haptic switches of the Trackpad
    /// panel are shown only where it can). Only macOS: Windows and Linux touchpads have no public
    /// haptic call.
    pub const fn has_haptics(self) -> bool {
        matches!(self, Os::Mac)
    }

    /// Whether this build shows a tray / menu bar icon at all. The first Linux release has none:
    /// Settings opens by holding the trigger in the center of the pie, and quits from its footer.
    pub const fn has_tray(self) -> bool {
        !matches!(self, Os::Linux)
    }

    /// What "Launch at login" does on this OS, shown under the checkbox.
    pub const fn startup_hint(self) -> &'static str {
        match self {
            Os::Mac => "Adds a LaunchAgent for your user. It takes effect at your next login.",
            Os::Windows => {
                "CursorPie is installed for your user in %LOCALAPPDATA%\\Programs\\CursorPie and starts when you sign in to Windows. Remove it from Settings > Apps."
            }
            Os::Linux => {
                "Adds ~/.config/autostart/cursorpie.desktop for your user. It takes effect at your next login."
            }
        }
    }

    /// Short version for the narrow Startup card in the welcome guide. The full path and details
    /// remain available through [`startup_hint`](Self::startup_hint) in Settings.
    pub const fn welcome_startup_hint(self) -> &'static str {
        match self {
            Os::Mac => "Adds a LaunchAgent for your user. It takes effect at your next login.",
            Os::Windows => "Starts when you sign in. Uninstall it from Settings > Apps.",
            Os::Linux => {
                "Adds an autostart entry for your user. It takes effect at your next login."
            }
        }
    }

    /// Placeholder of the "app id" text field of a profile.
    pub const fn app_id_hint(self) -> &'static str {
        match self {
            Os::Mac => "bundle id, e.g. com.apple.Safari",
            Os::Windows => "exe name, e.g. chrome.exe",
            Os::Linux => "desktop id, e.g. firefox",
        }
    }

    /// Placeholder of the text field of an "Open app" action.
    pub const fn open_app_hint(self) -> &'static str {
        match self {
            Os::Mac => "Safari  or  /Applications/X.app",
            Os::Windows => "chrome  or  C:\\Path\\App.exe",
            Os::Linux => "firefox  or  /usr/bin/app",
        }
    }

    /// Whether two app ids name the same app. Bundle ids are compared exactly (as before Windows
    /// support); executable names are not case sensitive.
    pub fn app_ids_equal(self, a: &str, b: &str) -> bool {
        match self {
            Os::Mac => a == b,
            Os::Windows => a.eq_ignore_ascii_case(b),
            Os::Linux => linux::desktop_entry::id_key(a) == linux::desktop_entry::id_key(b),
        }
    }

    /// Canonical form of an app id typed or picked by the user: trimmed, and on Windows lower case
    /// (the form [`frontmost_bundle_id`] reports).
    pub fn normalize_app_id(self, s: &str) -> String {
        let t = s.trim();
        match self {
            Os::Mac => t.to_string(),
            Os::Windows => t.to_ascii_lowercase(),
            Os::Linux => linux::desktop_entry::id_key(t),
        }
    }
}

/// [`Os::current`] as a constant.
pub const OS: Os = Os::current();
/// [`Os::primary_mod`] of this build.
pub const MOD_PRIMARY: u8 = OS.primary_mod();

/// File extensions (lower case) of the images CursorPie can decode, which is what the `image` crate
/// is built with (see `Cargo.toml`). One list for the file choosers, the icons folder scan and the
/// drag and drop, so they cannot disagree.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "ico", "tif", "tiff", "tga", "qoi", "pbm", "pgm",
    "ppm", "pnm",
];

/// Whether `ext` (any case, no dot) is one of [`IMAGE_EXTENSIONS`].
pub fn is_image_extension(ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    IMAGE_EXTENSIONS.contains(&ext.as_str())
}

/// What a file chooser should accept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// the extensions in [`IMAGE_EXTENSIONS`]
    Image,
    /// a JSON file (config or profile)
    Json,
    /// an application: an `.app` bundle on macOS, an `.exe` on Windows, a `.desktop` file on Linux
    App,
}

/// The icon of an app as the OS hands it out.
pub enum NativeIcon {
    /// A multi-page TIFF (macOS). Decoded by [`ui::appicon`](crate::ui::appicon).
    Tiff(Vec<u8>),
    /// Straight (not premultiplied) RGBA, `width * height * 4` bytes.
    Rgba {
        width: u32,
        height: u32,
        data: Vec<u8>,
    },
}

/// A haptic pattern for a trackpad (macOS: `NSHapticFeedbackManager`; other systems ignore it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HapticKind {
    /// A firm confirmation: "it is open".
    LevelChange,
    /// A soft detent: the hovered slice changed.
    Alignment,
    /// A plain click: the mode switched.
    Generic,
}

/// A display's bounds in logical points: `(x, y, width, height)`, in the same space as the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Starts a process and reaps its exit status on a tiny thread, so it never becomes a zombie.
///
/// The diagnostic log gets the program's name (never its arguments: they hold the person's command,
/// URL or path), whether it could be started, and how it ended.
pub fn spawn_reaped(cmd: &mut Command) {
    match cmd.spawn() {
        Ok(mut child) => {
            let program = cmd.get_program().to_string_lossy().into_owned();
            std::thread::spawn(move || {
                let status = child.wait();
                diag::log!(
                    "act",
                    "exited",
                    program = program,
                    code = status.ok().and_then(|s| s.code())
                );
            });
        }
        Err(e) => diag::log!(
            "act",
            "spawn_failed",
            program = cmd.get_program().to_string_lossy().into_owned(),
            error = format!("{:?}", e.kind())
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_primary_modifier_differs_per_os() {
        assert_eq!(Os::Mac.primary_mod(), MOD_CMD);
        assert_eq!(Os::Windows.primary_mod(), MOD_CTRL);
        assert_eq!(MOD_PRIMARY, OS.primary_mod());
        assert_eq!(Os::Linux.primary_mod(), MOD_CTRL);
    }

    #[test]
    fn linux_names_and_hints() {
        let l = Os::Linux.mod_names();
        assert_eq!(
            (l.ctrl, l.alt, l.shift, l.cmd),
            ("Ctrl", "Alt", "Shift", "Super")
        );
        assert_eq!(Os::Linux.primary_name(), "Ctrl");
        assert!(!Os::Linux.needs_permission());
        assert_eq!(Os::Linux.trigger_buttons(), &[2]);
        assert!(!Os::Linux.has_tray());
        assert!(Os::Mac.has_tray() && Os::Windows.has_tray());
        // a Mac trackpad, a Windows precision touchpad and a Linux (X11) touchpad can open the pie
        // by touch (docs/TRACKPAD_HAPTICS_PLAN.md); only the Mac one ticks
        assert!(Os::Mac.has_trackpad() && Os::Windows.has_trackpad() && Os::Linux.has_trackpad());
        assert!(Os::Mac.has_haptics());
        assert!(!Os::Windows.has_haptics() && !Os::Linux.has_haptics());
        assert!(Os::Linux.startup_hint().contains("autostart"));
        assert!(Os::Linux.app_id_hint().contains("desktop id"));
    }

    #[test]
    fn linux_app_ids_ignore_case_and_the_desktop_suffix() {
        assert!(Os::Linux.app_ids_equal("Firefox", "firefox"));
        assert!(Os::Linux.app_ids_equal("firefox.desktop", "FIREFOX"));
        assert!(!Os::Linux.app_ids_equal("firefox", "org.mozilla.firefox"));
        assert_eq!(
            Os::Linux.normalize_app_id("  Google-Chrome.desktop "),
            "google-chrome"
        );
    }

    #[test]
    fn modifier_names() {
        let m = Os::Mac.mod_names();
        assert_eq!((m.alt, m.cmd), ("Opt", "Cmd"));
        let w = Os::Windows.mod_names();
        assert_eq!((w.alt, w.cmd), ("Alt", "Win"));
        assert_eq!(Os::Mac.primary_name(), "Cmd");
        assert_eq!(Os::Windows.primary_name(), "Ctrl");
    }

    #[test]
    fn only_macos_needs_a_permission_and_extra_buttons() {
        assert!(Os::Mac.needs_permission());
        assert!(!Os::Windows.needs_permission());
        assert_eq!(Os::Mac.trigger_buttons(), [2, 5, 6, 7]);
        assert_eq!(Os::Windows.trigger_buttons(), [2]);
        // every offered button must survive Config::sanitize
        for os in [Os::Mac, Os::Windows] {
            assert!(
                os.trigger_buttons()
                    .iter()
                    .all(|b| matches!(b, 2 | 5 | 6 | 7))
            );
        }
    }

    #[test]
    fn app_ids_compare_per_os() {
        assert!(Os::Mac.app_ids_equal("com.apple.Safari", "com.apple.Safari"));
        assert!(!Os::Mac.app_ids_equal("com.apple.Safari", "com.apple.safari"));
        assert!(Os::Windows.app_ids_equal("Chrome.exe", "chrome.exe"));
        assert!(!Os::Windows.app_ids_equal("chrome.exe", "firefox.exe"));
    }

    #[test]
    fn app_ids_are_normalized_per_os() {
        assert_eq!(
            Os::Mac.normalize_app_id("  com.apple.Safari "),
            "com.apple.Safari"
        );
        assert_eq!(Os::Windows.normalize_app_id("  Chrome.EXE "), "chrome.exe");
        assert_eq!(Os::Windows.normalize_app_id("   "), "");
    }

    #[test]
    fn the_desktop_choices_differ_per_os() {
        assert_eq!(
            Os::Mac.desktop_modes(),
            [
                DesktopMode::SwitchToApp,
                DesktopMode::BringToCurrent,
                DesktopMode::DoNothing
            ]
        );
        assert_eq!(
            Os::Windows.desktop_modes(),
            [DesktopMode::SwitchToApp, DesktopMode::DoNothing]
        );
        assert_eq!(Os::Linux.desktop_modes(), Os::Windows.desktop_modes());
        // the default must be offered everywhere
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            assert!(os.desktop_modes().contains(&DesktopMode::default()));
        }
    }

    #[test]
    fn the_startup_hint_names_what_each_os_does() {
        assert!(Os::Mac.startup_hint().contains("LaunchAgent"));
        assert!(Os::Windows.startup_hint().contains("Programs"));
        assert!(Os::Windows.startup_hint().contains("sign in"));
    }

    #[test]
    fn the_labels_name_the_right_tools() {
        assert_eq!(Os::Mac.file_manager(), "Finder");
        assert_eq!(Os::Windows.file_manager(), "Explorer");
        assert_eq!(Os::Mac.tray_name(), "menu bar");
        assert_eq!(Os::Windows.tray_name(), "system tray");
        assert!(Os::Windows.app_id_hint().contains(".exe"));
        assert!(Os::Mac.app_id_hint().contains("com.apple"));
    }
}
