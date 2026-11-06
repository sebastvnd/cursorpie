//! Tray icon (the menu bar on macOS, the notification area on Windows): Settings, About, Welcome
//! guide, Pause, Launch at login, Quit.
//!
//! tray-icon must be created on the main thread once the event loop is running; eframe calls the
//! app creator from `resumed`, which satisfies that on both platforms. On other targets (the Linux
//! test stub) there is no tray and [`build`] returns `None`.

use crate::msg::Notifier;

#[cfg(not(any(target_os = "macos", windows)))]
pub use none::*;
#[cfg(any(target_os = "macos", windows))]
pub use real::*;

/// RGB color of the Windows tray icon (`#728B73`, the site and app sage accent). macOS draws a
/// black template image that the system recolors; Windows has no template images, so the icon is
/// drawn in sage on a transparent background.
pub const WINDOWS_ICON_RGB: [u8; 3] = [114, 139, 115];

/// The four-segment CursorPie mark as an RGBA image: `rgb` color and a coverage-based alpha.
/// The center is a diamond, matching the app icon and the brand artwork.
pub fn icon_rgba(n: u32, rgb: [u8; 3]) -> Vec<u8> {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
    let mut px = vec![0u8; (n * n * 4) as usize];
    let c = n as f32 / 2.0;
    for y in 0..n {
        for x in 0..n {
            let dx = x as f32 + 0.5 - c;
            let dy = y as f32 + 0.5 - c;
            let d = dx.hypot(dy) / c;
            let ang = dy.atan2(dx);
            // gaps along the four diagonals
            let from_diag = (ang.rem_euclid(FRAC_PI_2) - FRAC_PI_4).abs();
            let in_gap = from_diag < 0.11;

            let ring = ((0.94 - d) * c).clamp(0.0, 1.0) * ((d - 0.50) * c).clamp(0.0, 1.0);
            let ring = if in_gap { 0.0 } else { ring };
            // A Manhattan-distance threshold gives the center its diamond shape.
            let diamond = ((0.30 - (dx.abs() + dy.abs()) / c) * c).clamp(0.0, 1.0);
            let alpha = ring.max(diamond);

            let i = ((y * n + x) * 4) as usize;
            px[i..i + 3].copy_from_slice(&rgb);
            px[i + 3] = (alpha * 255.0) as u8;
        }
    }
    px
}

#[cfg(any(target_os = "macos", windows))]
mod real {
    use super::*;
    use crate::about::APP_NAME;
    use crate::msg::Msg;
    use crate::platform;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

    /// Tray handles the app keeps: the icon (dropping it removes it) and the two check items it updates.
    pub struct TrayHandles {
        /// keep alive: dropping it removes the icon
        pub _icon: TrayIcon,
        pause: CheckMenuItem,
        login: CheckMenuItem,
    }

    impl TrayHandles {
        /// Whether "Pause" is ticked.
        pub fn pause_checked(&self) -> bool {
            self.pause.is_checked()
        }
        /// Whether "Launch at login" is ticked.
        pub fn login_checked(&self) -> bool {
            self.login.is_checked()
        }
        /// Ticks or unticks "Launch at login".
        pub fn set_login_checked(&self, on: bool) {
            self.login.set_checked(on);
        }
    }

    /// Creates the tray icon and its menu. Menu clicks are forwarded as `Msg::Tray(id)`.
    /// Returns `None` (after logging) if the icon cannot be created.
    pub fn build(n: &Notifier) -> Option<TrayHandles> {
        let menu = Menu::new();
        let settings = MenuItem::with_id("settings", "Settings...", true, None);
        let about = MenuItem::with_id("about", format!("About {APP_NAME}"), true, None);
        let welcome = MenuItem::with_id("welcome", "Welcome guide...", true, None);
        let swap = MenuItem::with_id("swap_side", "Switch side-button pie", true, None);
        let pause = CheckMenuItem::with_id("pause", format!("Pause {APP_NAME}"), true, false, None);
        let login = CheckMenuItem::with_id(
            "login",
            "Launch at login",
            true,
            platform::launch_at_login_enabled(),
            None,
        );
        let quit = MenuItem::with_id("quit", format!("Quit {APP_NAME}"), true, None);

        let _ = menu.append(&settings);
        let _ = menu.append(&about);
        let _ = menu.append(&welcome);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&swap);
        let _ = menu.append(&pause);
        let _ = menu.append(&login);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&quit);

        // forward menu clicks to the UI thread (this also wakes it up)
        let notifier = n.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            notifier.send(Msg::Tray(e.id.0.clone()));
        }));

        let rgb = match platform::OS {
            platform::Os::Mac => [0, 0, 0],
            platform::Os::Windows | platform::Os::Linux => WINDOWS_ICON_RGB,
        };
        let icon = match Icon::from_rgba(icon_rgba(44, rgb), 44, 44) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("CursorPie: could not create tray icon image: {e}");
                return None;
            }
        };
        match TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(APP_NAME)
            .with_icon(icon)
            .with_icon_as_template(true) // macOS only; ignored on Windows
            .build()
        {
            Ok(tray) => Some(TrayHandles {
                _icon: tray,
                pause,
                login,
            }),
            Err(e) => {
                eprintln!(
                    "CursorPie: could not create the {} icon: {e}",
                    platform::OS.tray_name()
                );
                None
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod none {
    use super::*;

    /// There is no tray on this target; the handles never exist.
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

    /// No tray on this target.
    pub fn build(_n: &Notifier) -> Option<TrayHandles> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_a_ring_with_a_diamond_and_four_gaps() {
        let n = 44u32;
        let px = icon_rgba(n, [1, 2, 3]);
        assert_eq!(px.len(), (n * n * 4) as usize);
        let alpha = |x: u32, y: u32| px[((y * n + x) * 4 + 3) as usize];
        assert!(alpha(22, 22) > 200, "the center of the diamond");
        assert!(alpha(26, 22) > 200, "the diamond reaches its four tips");
        assert_eq!(alpha(27, 27), 0, "the diamond corners are outside the mark");
        assert_eq!(alpha(0, 0), 0, "outside the ring");
        assert!(alpha(40, 22) > 200, "the ring on the right");
        assert_eq!(&px[0..3], &[1, 2, 3], "every pixel carries the color");
        // a diagonal gap: the ring is cut at 45 degrees
        assert_eq!(alpha(22 + 14, 22 + 14), 0);
    }
}
