//! Linux support. See `docs/developer/LINUX_MIGRATION.md` for the design.
//!
//! The pure helpers (session detection, desktop entries, icon themes, window hit test, XDG paths, autostart file
//! text, launch resolution, app roles, display scale, the kernel's list of input devices) compile on every OS, so their tests run everywhere. The X11
//! backend (`xlib`, `x11`, `backend`) is only compiled on Linux. The Wayland backend (`wayland`, `uinput`) has its pure parts (the clone's description, the KWin script and its reports) built everywhere too; see `docs/developer/WAYLAND_SUPPORT.md`.

pub mod autostart;
// The touchpad source (the kernel's evdev nodes): its pure parser is tested on every OS.
pub mod desktop_entry;
#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) mod evdev;
pub(crate) mod evdev_io;
pub mod icontheme;
pub mod inputdev;
pub mod launch;
pub mod procmem;
pub mod refresh;
pub mod roles;
pub mod scale;
pub mod session;
pub mod toplevels;
pub(crate) mod uinput;
pub(crate) mod wayland;
pub mod xdg;

#[cfg(target_os = "linux")]
pub(crate) mod backend;
#[cfg(target_os = "linux")]
pub(crate) mod pinch;
#[cfg(target_os = "linux")]
pub(crate) use crate::platform::unix_process as process;
#[cfg(target_os = "linux")]
pub(crate) mod tray;
#[cfg(target_os = "linux")]
pub(crate) mod x11;
#[cfg(target_os = "linux")]
pub(crate) mod xlib;

/// A lookup of environment variables from a fixed list of pairs, for the tests of the pieces that
/// read the session (`session`, `wayland::sway`, `wayland::hyprland`): they take the lookup as a
/// function so no test touches the real environment.
#[cfg(test)]
pub(crate) fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |k| {
        pairs
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.to_string())
    }
}
