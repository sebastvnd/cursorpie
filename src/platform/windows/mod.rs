//! Windows backend: Win32 through `windows-sys`.
//!
//! The code deliberately avoids `std::os::windows`, so it type-checks on any OS with
//! `cargo check --features check-windows`, and everything that is a pure decision (path to app id,
//! DPI math, command lines, filter strings, key tables) lives in [`util`] and the pure parts of
//! the other modules, where unit tests run everywhere.
//!
//! | Submodule | Contents |
//! |---|---|
//! | `util` | wide strings, app ids from paths, DPI math, command lines (pure) |
//! | `dpi` | monitor scale lookup: the bridge between physical pixels and logical points |
//! | `apps` | frontmost app, app under the pointer, app icons, activation, cursor, displays |
//! | `apppath` | where the program of an app id is (App Paths, Start menu, program folders) and the list of installed apps |
//! | `shellitem` | the shell's icon of a file (`IShellItemImageFactory`) and the target of a `.lnk` (`IShellLinkW`) |
//! | `desktops` | "Open app" that follows the app's window to its virtual desktop (`IVirtualDesktopManager` and Win+Ctrl+Arrows) |
//! | `input` | `SendInput` for shortcuts and replayed mouse buttons |
//! | `layout` | keyboard-layout snapshot (`ToUnicodeEx`) |
//! | `shell` | `ShellExecuteW`, the file chooser, `explorer`, beep |
//! | `registry` | `HKCU` helpers shared by the two modules below |
//! | `login_item` | "Launch at login" registry value |
//! | `install` | per-user install, "Apps & features" entry, `--uninstall` |
//! | `instance` | one running copy at a time |
//! | `window` | makes the one native window truly frameless (no white box, border or shadow) |
//!
//! There is no permission to ask for on Windows, and nothing like a Dock icon to hide.

#![cfg_attr(not(windows), allow(unused_imports))]

pub mod layout;
pub mod util;

mod apppath;
mod apps;
mod desktops;
mod dpi;
mod input;
mod install;
mod instance;
mod login_item;
mod registry;
mod shell;
mod shellitem;
mod window;

pub use apppath::installed_list;
pub use apps::*;
pub use desktops::*;
pub use input::*;
pub use install::*;
pub use instance::*;
pub use login_item::*;
pub use shell::*;
pub use window::*;

pub(crate) use dpi::{scale_at_physical, set_last_scale};
pub(crate) use util::px_to_logical;

use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};

/// Always `true`: input hooks and `SendInput` need no permission.
pub fn is_trusted() -> bool {
    true
}

/// Always `true`.
pub fn is_trusted_cached() -> bool {
    true
}

/// There is no Accessibility pane on Windows.
pub fn open_accessibility_settings() {}

/// There is no Input Monitoring pane on Windows.
pub fn open_input_monitoring_settings() {}

/// There is no Dock icon to hide (the window is kept off the taskbar by the window builder).
pub fn set_accessory_policy() {}

/// Asks Windows to trim the working set, which returns freed pages to the system. Reports 0 because
/// Windows does not say how much was released.
pub fn release_unused_memory() -> usize {
    // SAFETY: the pseudo handle of the current process is always valid; (-1, -1) means "trim".
    unsafe {
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
    0
}
