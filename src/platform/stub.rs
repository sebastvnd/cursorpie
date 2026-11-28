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
use crate::windowing::{Asked, Windows};
use std::path::Path;

/// Only Hyprland places the window through the compositor.
pub fn place_window_center(_top_left: (f64, f64), _size: (f64, f64)) -> bool {
    false
}
/// Nothing is being placed.
pub fn window_placement_pending() -> bool {
    false
}
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
/// There is no window.
pub fn set_window_passive(_on: bool) {}
/// There is no window.
pub fn keep_over_full_screen(_over: bool) {}
/// Nothing moves the window.
pub fn window_manager_may_recenter() -> bool {
    false
}
/// A window is placed in the same points the pointer is in.
pub fn window_point(x: f64, y: f64) -> (f64, f64) {
    (x, y)
}

/// The pie is drawn in the same points the pointer moves in.
pub fn window_scale(_x: f64, _y: f64) -> f64 {
    1.0
}

/// There is no window.
pub fn raise_own_window() {}
/// No mice to list.
pub fn mouse_devices() -> Vec<crate::mouse::Device> {
    Vec::new()
}
/// No pointer.
pub fn cursor_pos() -> Option<(f64, f64)> {
    None
}
/// No pointer.
pub fn cursor_pos_fresh() -> Option<(f64, f64)> {
    None
}
/// No pointer to ask for.
pub fn pointer_hot(_on: bool) {}

/// Only macOS has apps that hide the pointer until the mouse moves.
pub fn reveal_cursor() {}

/// Only a Wayland session shows a black square when a transparent window is first mapped.
pub fn set_window_veil(_window: std::os::raw::c_ulong, _on: bool) -> bool {
    false
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
pub fn open_app_new_window(_app: &str) -> Asked {
    Asked::NotSent
}
/// Windows cannot be counted.
pub fn window_count(_app: &str) -> Option<usize> {
    None
}
/// Windows cannot be told apart.
pub fn window_ids(_app: &str) -> Option<Windows> {
    None
}
/// Ignored.
pub fn place_new_windows_on_pointer_display(_app: &str) {}

/// No app is known to have a single window.
pub fn declares_single_window(_app: &str) -> bool {
    false
}
/// Nothing runs: no app lookup.
pub fn running_among(apps: &[String]) -> Vec<bool> {
    vec![false; apps.len()]
}
/// Ignored.
pub fn open_url(_target: &str) {}
/// Nothing to wait for: the login entry already starts the app when the session is ready.
pub fn wait_for_login_ready() {}

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
/// No haptics.
pub fn haptic(_kind: crate::platform::HapticKind) {}

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
/// No administrator mode.
pub fn admin_mode() -> crate::platform::AdminMode {
    crate::platform::AdminMode::default()
}
/// No administrator mode.
pub fn reinstall_as_administrator() -> Result<(), crate::platform::ReinstallError> {
    Err(crate::platform::ReinstallError::Unsupported)
}
/// Nothing throttles it.
pub fn stay_responsive() {}
/// Nothing to release.
pub fn release_unused_memory() -> usize {
    0
}
/// The memory is not measured.
pub fn memory_use() -> Option<crate::platform::MemoryUse> {
    None
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

// ------------------------------------------------------------------------ helpers of the other backends

/// Nothing to hide.
pub fn hide_console(_cmd: &mut std::process::Command) {}
/// Ends `child`.
pub fn terminate_tree(child: &mut std::process::Child) {
    let _ = child.kill();
}
/// `-c line`, as in a Unix shell.
pub fn shell_line(cmd: &mut std::process::Command, line: &str) {
    cmd.arg("-c").arg(line);
}
/// Nothing is detached.
pub fn detach_from_parent(_cmd: &mut std::process::Command) {}
/// No permission bits.
pub fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
/// A plain file.
pub fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}
/// `ENOSPC`, as on the Unix systems.
pub fn is_disk_full(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(28)
}
/// Nothing is installed.
pub fn collect_installed_apps() -> Vec<crate::platform::installed::InstalledApp> {
    Vec::new()
}
/// The link is opened as usual.
pub fn open_url_new_window(target: &str) {
    open_url(target);
}
/// Nothing to raise.
pub fn raise_browser(_browser: &str) {}
/// winit's own choice of window system.
pub fn event_loop_builder_hook() -> Option<eframe::EventLoopBuilderHook> {
    None
}

/// There is no tray here; the handles never exist.
pub struct TrayHandles;

impl TrayHandles {
    /// Never ticked.
    pub fn pause_checked(&self) -> bool {
        false
    }
    /// Never ticked.
    pub fn login_checked(&self) -> bool {
        false
    }
    /// Ignored.
    pub fn set_login_checked(&self, _on: bool) {}
}

/// No tray here.
pub fn build_tray(_n: &crate::msg::Notifier) -> Option<TrayHandles> {
    None
}

/// No touch source.
pub(crate) fn start_touch_source(
    _state: std::sync::Arc<std::sync::Mutex<crate::tap::gesture::Gesture>>,
    _notifier: crate::msg::Notifier,
) {
}
/// No touchpads.
pub fn probe_trackpads() -> usize {
    0
}
