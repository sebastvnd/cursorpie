//! Linux support. See `docs/LINUX_MIGRATION.md` for the design.
//!
//! The pure helpers (session detection, desktop entries, icon themes, window hit test, XDG paths, autostart file
//! text, launch resolution, app roles, display scale, the kernel's list of input devices) compile on every OS, so their tests run everywhere. The X11
//! backend (`xlib`, `x11`, `backend`) is only compiled on Linux.

pub mod autostart;
pub mod desktop_entry;
pub(crate) mod evdev_io;
pub mod icontheme;
pub mod inputdev;
pub mod launch;
pub mod roles;
pub mod scale;
pub mod session;
pub mod toplevels;
pub mod xdg;

#[cfg(target_os = "linux")]
pub(crate) mod backend;
#[cfg(target_os = "linux")]
pub(crate) mod x11;
#[cfg(target_os = "linux")]
pub(crate) mod xlib;
