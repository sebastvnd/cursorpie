//! Where to put the pie window in a Wayland session (decision D7 of `docs/developer/WAYLAND_SUPPORT.md`).
//!
//! The pointer and the outputs come from KWin, in compositor points. The pie is still a window
//! of the X11 toolkit, which runs through XWayland and places it in *its* points: X pixels divided
//! by the toolkit's scale (what `Conn::monitors` of the X11 backend reports). The two agree
//! when KDE's scale and the X11 scale are the same number, and differ otherwise (fractional scales,
//! and the setting that scales X11 applications by the system), which puts the pie off the cursor.
//!
//! So the position is not taken over as it is: it is mapped from the KWin output it is on to the X
//! monitor of the same screen, linearly, which is exact whatever the two scales are and does not
//! assume anything about them. The same ratio turns a distance of the pointer into a distance in
//! the pie, which is drawn in the toolkit's points ([`scale_at`]). Pure, so it is tested on every OS.

use crate::platform::ScreenRect;

/// How far the horizontal and the vertical ratio of a pair may differ (2%): a pair that is further
/// apart is not the same screen, and nothing is converted.
const ASPECT_TOLERANCE: f64 = 0.02;

/// How far `(x, y)` is from `r`; 0 inside it.
fn distance(r: &ScreenRect, x: f64, y: f64) -> f64 {
    let dx = (r.x - x).max(0.0).max(x - (r.x + r.w));
    let dy = (r.y - y).max(0.0).max(y - (r.y + r.h));
    dx.hypot(dy)
}

/// A rectangle with finite numbers and a size.
fn usable(r: &ScreenRect) -> bool {
    [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) && r.w >= 1.0 && r.h >= 1.0
}

/// The rectangles from left to right, then top to bottom: the same screens in the same order in
/// both lists, whatever order each system lists them in.
fn in_order(list: &[ScreenRect]) -> Vec<ScreenRect> {
    let mut v = list.to_vec();
    v.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    v
}

/// The KWin output a point is on, the X monitor of the same screen, and the ratio between them.
struct Matched {
    out: ScreenRect,
    mon: ScreenRect,
    sx: f64,
    sy: f64,
}

/// The output `p` is on and the X monitor that goes with it. `None` when the two lists cannot be
/// matched (a different number of screens, a screen of another shape, a bad number).
fn matched(p: (f64, f64), outputs: &[ScreenRect], x_monitors: &[ScreenRect]) -> Option<Matched> {
    if outputs.is_empty()
        || outputs.len() != x_monitors.len()
        || !outputs.iter().chain(x_monitors).all(usable)
    {
        return None;
    }
    let outs = in_order(outputs);
    let mons = in_order(x_monitors);
    // a point on no output (the edge, or an output that just went away) goes with the nearest
    let (i, out) = outs
        .iter()
        .enumerate()
        .min_by(|a, b| distance(a.1, p.0, p.1).total_cmp(&distance(b.1, p.0, p.1)))?;
    let mon = mons[i];
    let (sx, sy) = (mon.w / out.w, mon.h / out.h);
    if (sx / sy - 1.0).abs() > ASPECT_TOLERANCE {
        return None;
    }
    Some(Matched {
        out: *out,
        mon,
        sx,
        sy,
    })
}

/// `p` (compositor points) in the points the toolkit's window is placed in. `outputs` are the
/// KWin outputs, `x_monitors` the X monitors in the toolkit's points. `None` when the two lists
/// cannot be matched (a different number of screens, a screen of another shape, a bad number):
/// the caller then keeps `p`, which is what was done before this existed.
pub(crate) fn to_window_point(
    p: (f64, f64),
    outputs: &[ScreenRect],
    x_monitors: &[ScreenRect],
) -> Option<(f64, f64)> {
    let m = matched(p, outputs, x_monitors)?;
    Some((
        m.mon.x + (p.0 - m.out.x) * m.sx,
        m.mon.y + (p.1 - m.out.y) * m.sy,
    ))
}

/// How many of the toolkit's points one compositor point is at `p`: 1.5 when KWin counts 2560 points
/// across a screen that X shows as 3840. The same screens as [`to_window_point`], and `None` for
/// the same reasons; the caller then takes 1.
///
/// The pie is drawn in the toolkit's points, and the pointer moves in the compositor's: a distance
/// from the center of the pie has to be converted before it is compared with the size of a slice,
/// or the slice that lights up is not the one the pointer is on.
pub(crate) fn scale_at(
    p: (f64, f64),
    outputs: &[ScreenRect],
    x_monitors: &[ScreenRect],
) -> Option<f64> {
    let m = matched(p, outputs, x_monitors)?;
    Some((m.sx * m.sy).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> ScreenRect {
        ScreenRect { x, y, w, h }
    }

    fn close(a: (f64, f64), b: (f64, f64)) {
        assert!(
            (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9,
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn the_same_scale_on_both_sides_changes_nothing() {
        let screen = [r(0.0, 0.0, 1920.0, 1080.0)];
        let p = to_window_point((960.0, 540.0), &screen, &screen).unwrap();
        close(p, (960.0, 540.0));
    }

    #[test]
    fn an_x11_scale_of_one_on_a_fractional_screen_is_converted() {
        // KDE at 150%: 2560 x 1440 points of 3840 x 2160 pixels; X11 not scaled, so its points
        // are pixels
        let kwin = [r(0.0, 0.0, 2560.0, 1440.0)];
        let x = [r(0.0, 0.0, 3840.0, 2160.0)];
        close(
            to_window_point((1280.0, 720.0), &kwin, &x).unwrap(),
            (1920.0, 1080.0),
        );
        close(to_window_point((0.0, 0.0), &kwin, &x).unwrap(), (0.0, 0.0));
        close(
            to_window_point((100.0, 50.0), &kwin, &x).unwrap(),
            (150.0, 75.0),
        );
    }

    #[test]
    fn a_second_screen_keeps_its_own_offset_and_ratio() {
        // two screens of different scales; X places the second one after the first in its own
        // pixels, not at the KWin offset
        let kwin = [r(0.0, 0.0, 1920.0, 1080.0), r(1920.0, 0.0, 1280.0, 720.0)];
        let x = [r(0.0, 0.0, 3840.0, 2160.0), r(3840.0, 0.0, 1280.0, 720.0)];
        close(
            to_window_point((1920.0 + 640.0, 360.0), &kwin, &x).unwrap(),
            (3840.0 + 640.0, 360.0),
        );
        close(
            to_window_point((960.0, 540.0), &kwin, &x).unwrap(),
            (1920.0, 1080.0),
        );
    }

    #[test]
    fn the_lists_may_come_in_any_order() {
        let kwin = [r(1920.0, 0.0, 1280.0, 720.0), r(0.0, 0.0, 1920.0, 1080.0)];
        let x = [r(3840.0, 0.0, 1280.0, 720.0), r(0.0, 0.0, 3840.0, 2160.0)];
        close(
            to_window_point((960.0, 540.0), &kwin, &x).unwrap(),
            (1920.0, 1080.0),
        );
    }

    #[test]
    fn a_point_off_every_screen_goes_with_the_nearest_one() {
        let kwin = [r(0.0, 0.0, 100.0, 100.0), r(100.0, 0.0, 100.0, 100.0)];
        let x = [r(0.0, 0.0, 200.0, 200.0), r(200.0, 0.0, 200.0, 200.0)];
        // 10 points left of the first screen
        close(
            to_window_point((-10.0, 50.0), &kwin, &x).unwrap(),
            (-20.0, 100.0),
        );
    }

    #[test]
    fn the_scale_is_how_many_toolkit_points_a_compositor_point_is() {
        let kwin = [r(0.0, 0.0, 2560.0, 1440.0)];
        let x = [r(0.0, 0.0, 3840.0, 2160.0)];
        let s = scale_at((100.0, 100.0), &kwin, &x).unwrap();
        assert!((s - 1.5).abs() < 1e-9, "{s}");
        // equal scales: 1
        let same = [r(0.0, 0.0, 1920.0, 1080.0)];
        assert_eq!(scale_at((5.0, 5.0), &same, &same), Some(1.0));
    }

    #[test]
    fn each_screen_has_its_own_scale() {
        let kwin = [r(0.0, 0.0, 1920.0, 1080.0), r(1920.0, 0.0, 1280.0, 720.0)];
        let x = [r(0.0, 0.0, 3840.0, 2160.0), r(3840.0, 0.0, 1280.0, 720.0)];
        assert_eq!(scale_at((10.0, 10.0), &kwin, &x), Some(2.0));
        assert_eq!(scale_at((2000.0, 10.0), &kwin, &x), Some(1.0));
    }

    #[test]
    fn a_scale_is_not_made_up_for_screens_that_cannot_be_matched() {
        let one = [r(0.0, 0.0, 100.0, 100.0)];
        let two = [r(0.0, 0.0, 100.0, 100.0), r(100.0, 0.0, 100.0, 100.0)];
        assert_eq!(scale_at((5.0, 5.0), &one, &two), None);
        assert_eq!(scale_at((5.0, 5.0), &[], &[]), None);
        let wide = [r(0.0, 0.0, 200.0, 100.0)];
        assert_eq!(scale_at((5.0, 5.0), &one, &wide), None);
    }

    #[test]
    fn what_cannot_be_matched_is_left_alone() {
        let one = [r(0.0, 0.0, 100.0, 100.0)];
        let two = [r(0.0, 0.0, 100.0, 100.0), r(100.0, 0.0, 100.0, 100.0)];
        // not the same number of screens
        assert_eq!(to_window_point((5.0, 5.0), &one, &two), None);
        assert_eq!(to_window_point((5.0, 5.0), &[], &[]), None);
        // a screen of another shape (not just another scale)
        let wide = [r(0.0, 0.0, 200.0, 100.0)];
        assert_eq!(to_window_point((5.0, 5.0), &one, &wide), None);
        // numbers that are not numbers, or no size
        let bad = [r(f64::NAN, 0.0, 100.0, 100.0)];
        assert_eq!(to_window_point((5.0, 5.0), &bad, &one), None);
        let flat = [r(0.0, 0.0, 0.0, 100.0)];
        assert_eq!(to_window_point((5.0, 5.0), &one, &flat), None);
    }
}
