//! Tray icon (the menu bar on macOS, the notification area on Windows): Settings, About, Welcome
//! guide, Pause, Launch at login, Quit.
//!
//! The menu and the icon are made by the backend of each OS (`platform::build_tray`, which needs
//! `tray-icon`); this file holds what is the same everywhere: the artwork. Where there is no tray
//! (Linux, the test stub) [`build`] returns `None`.

use crate::msg::Notifier;

pub use crate::platform::TrayHandles;

/// Creates the tray icon and its menu, `None` when this system has none or it cannot be created.
pub fn build(n: &Notifier) -> Option<TrayHandles> {
    crate::platform::build_tray(n)
}

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
