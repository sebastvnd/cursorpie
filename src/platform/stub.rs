//! Backend for operating systems CursorPie does not run on (for example Linux).
//!
//! It exists so the portable code (config, geometry, the gesture state machine, key tables) can be
//! built and unit tested on any machine. Nothing here does anything useful at runtime: queries
//! return `None` / `false` and actions are ignored.

use crate::config::DesktopMode;
use crate::geometry::Point;
use crate::keys::Shortcut;
use crate::platform::{FileKind, NativeIcon, ScreenRect};
use crate::tap::Kind;
use std::path::Path;

/// No app lookup.
pub fn frontmost_bundle_id() -> Option<String> {
    None
}
/// No app lookup.
pub fn bundle_id_at(_p: Point) -> Option<String> {
    None
}
/// No app lookup.
pub fn bundle_id_of(_app_path: &str) -> Option<String> {
    None
}
/// No icons.
pub fn app_icon(_bundle: &str) -> Option<NativeIcon> {
    None
}
/// Nothing to hide.
pub fn set_accessory_policy() {}
/// The window has no frame to remove.
pub fn clear_window_frame(_handle: isize) {}
/// Nothing moves the window.
pub fn window_manager_may_recenter() -> bool {
    false
}
/// No pointer.
pub fn cursor_pos() -> Option<(f64, f64)> {
    None
}
/// No displays.
pub fn display_rects() -> Vec<ScreenRect> {
    Vec::new()
}

/// Always `true`: there is no permission to ask for.
pub fn is_trusted() -> bool {
    true
}
/// Always `true`.
pub fn is_trusted_cached() -> bool {
    true
}
/// Nothing to open.
pub fn open_accessibility_settings() {}
/// Nothing to open.
pub fn open_input_monitoring_settings() {}

/// Ignored.
pub fn press_shortcut(_sc: Shortcut) {}
/// Ignored.
pub fn post_mouse(_pos: Point, _kind: Kind, _button: i64) {}
/// Ignored.
pub fn activate_app(_bundle: &str) {}

/// Ignored.
pub fn open_app(_app: &str) {}
/// Ignored.
pub fn reveal_app(_app: &str, _mode: DesktopMode) {}
/// Ignored.
pub fn open_url(_target: &str) {}
/// Ignored.
pub fn run_shell(_cmd: &str) {}
/// Ignored.
pub fn open_path(_path: &Path) {}
/// Ignored.
pub fn reveal_in_file_manager(_path: &Path) {}
/// No chooser.
pub fn choose_file(_kind: FileKind, _prompt: &str) -> Option<String> {
    None
}
/// Silent.
pub fn tick() {}

/// Never enabled.
pub fn launch_at_login_enabled() -> bool {
    false
}
/// Ignored.
pub fn set_launch_at_login(_on: bool) -> std::io::Result<()> {
    Ok(())
}
/// Nothing to install.
pub fn ensure_installed() -> Option<std::path::PathBuf> {
    None
}
/// No command-line switches.
pub fn handle_command_line() -> bool {
    false
}
/// Always the only one.
pub fn single_instance() -> bool {
    true
}
/// Nothing to release.
pub fn release_unused_memory() -> usize {
    0
}

/// Keyboard layout: there is no snapshot, so labels use the US/ANSI fallback table.
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
