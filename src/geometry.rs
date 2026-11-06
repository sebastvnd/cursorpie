//! Pie geometry: sizes and slice hit-testing.
//!
//! Pure math with no drawing and no OS types, shared by the event tap (which decides what is hovered) and the UI
//! (which draws it), so both always agree on where a slice is.
//!
//! Slice 0 is centered on the left (angle pi) and slices run clockwise on screen.
//!
//! ```
//! use cursorpie::geometry::{pie_inner, slice_at};
//! use cursorpie::geometry::Point;
//!
//! let center = Point::new(100.0, 100.0);
//! // inside the dead zone nothing is hovered
//! assert_eq!(slice_at(center, center, 4, pie_inner(200.0)), None);
//! // directly left of the center is slice 0
//! assert_eq!(slice_at(center, Point::new(20.0, 100.0), 4, pie_inner(200.0)), Some(0));
//! ```

use std::f32::consts::{PI, TAU};

/// A point in global screen space, in logical points (the unit egui uses, y grows downwards).
/// The native event layers convert to this so everything above them is OS independent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    /// A point at `(x, y)`.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Extra room, in points, the pie window has on every side beyond the ring's own square, for the
/// badges of an "Open app" slice (see [`running`](crate::running)), which sit outside the rim. The
/// window is transparent and click-through, so the room costs nothing to look at.
pub const BADGE_ROOM: f32 = 36.0;

/// Side of the (square) pie window for a pie of size `win` (`Config::pie_size`): the pie plus
/// [`BADGE_ROOM`] on every side. The ring is still drawn and hit-tested for `win`, around the center.
pub fn window_size(win: f32) -> f32 {
    win + 2.0 * BADGE_ROOM
}

/// Outer radius of the ring for a window of size `win`.
pub fn pie_outer(win: f32) -> f32 {
    win / 2.0 - 10.0
}
/// Inner radius (the dead zone around the center disc).
pub fn pie_inner(win: f32) -> f32 {
    pie_outer(win) * 0.36
}
/// Start angle (radians) of slice `i` out of `total`.
pub fn slice_start(i: usize, total: usize) -> f32 {
    let sl = TAU / total as f32;
    PI - sl / 2.0 + i as f32 * sl
}

/// Which slice is `p` over, relative to `center`? None inside the dead zone.
pub fn slice_at(center: Point, p: Point, total: usize, dead_zone: f32) -> Option<usize> {
    let dx = (p.x - center.x) as f32;
    let dy = (p.y - center.y) as f32;
    if dx.hypot(dy) < dead_zone || total == 0 {
        return None;
    }
    let mut angle = dy.atan2(dx);
    if angle < 0.0 {
        angle += TAU;
    }
    let slice_angle = TAU / total as f32;
    let mut adjusted = angle - slice_start(0, total);
    while adjusted < 0.0 {
        adjusted += TAU;
    }
    Some((adjusted / slice_angle).floor() as usize % total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn sizes_follow_the_window() {
        assert_eq!(pie_outer(200.0), 90.0);
        assert!((pie_inner(200.0) - 32.4).abs() < 1e-4);
    }

    #[test]
    fn four_slices_run_clockwise_from_the_left() {
        let c = at(100.0, 100.0);
        let dz = pie_inner(200.0);
        assert_eq!(slice_at(c, at(40.0, 100.0), 4, dz), Some(0), "left");
        assert_eq!(
            slice_at(c, at(100.0, 40.0), 4, dz),
            Some(1),
            "up (y grows downwards)"
        );
        assert_eq!(slice_at(c, at(160.0, 100.0), 4, dz), Some(2), "right");
        assert_eq!(slice_at(c, at(100.0, 160.0), 4, dz), Some(3), "down");
    }

    #[test]
    fn the_dead_zone_and_empty_rings_hover_nothing() {
        let c = at(0.0, 0.0);
        assert_eq!(slice_at(c, at(10.0, 0.0), 4, 32.4), None);
        assert_eq!(slice_at(c, at(50.0, 0.0), 0, 32.4), None);
    }

    #[test]
    fn eight_slices_cover_the_circle() {
        let c = at(0.0, 0.0);
        let mut seen = [false; 8];
        for deg in (0..360).step_by(5) {
            let a = (deg as f64).to_radians();
            let p = at(60.0 * a.cos(), 60.0 * a.sin());
            seen[slice_at(c, p, 8, 32.4).unwrap()] = true;
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn slice_zero_starts_half_a_slice_before_the_left() {
        let start = slice_start(0, 4);
        assert!((start - (PI - PI / 4.0)).abs() < 1e-6);
        assert!((slice_start(1, 4) - start - PI / 2.0).abs() < 1e-6);
    }
}
