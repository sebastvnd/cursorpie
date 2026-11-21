//! Display lookup (global coordinates in logical points, the same space as the cursor).

use crate::platform::{self, ScreenRect};
use eframe::egui::{self, Pos2};

/// Converts display bounds to an egui rectangle.
fn rect_of(b: ScreenRect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(b.x as f32, b.y as f32),
        egui::vec2(b.w as f32, b.h as f32),
    )
}

/// The display containing `p`, or the first one when `p` is on none of them. `None` without displays.
pub fn pick_display(displays: &[ScreenRect], p: Pos2) -> Option<egui::Rect> {
    let mut first = None;
    for d in displays {
        let r = rect_of(*d);
        if r.contains(p) {
            return Some(r);
        }
        first.get_or_insert(r);
    }
    first
}

/// What to do about the pie window's position right after it was shown, see [`pie_hold`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieHold {
    /// The window is where it was asked to be: nothing to send, keep watching for a moment, since
    /// the position read back can be the old one.
    Keep,
    /// The window manager put it elsewhere (or its position is unknown): ask for the position again.
    Move,
    /// The watch is over (time is up). A window manager that keeps the window somewhere else on
    /// purpose, such as at the edge of the screen, is not fought any longer.
    Release,
}

/// Decides what to do with the pie window `elapsed` after it was shown at `target` (its top-left
/// corner), given where the window really is (`outer_min`, `None` if unknown). A position closer
/// than one point counts as the right one: the window sits on whole physical pixels.
pub fn pie_hold(
    target: Pos2,
    outer_min: Option<Pos2>,
    elapsed: std::time::Duration,
    hold: std::time::Duration,
) -> PieHold {
    if elapsed > hold {
        PieHold::Release
    } else if outer_min.is_some_and(|p| (p - target).abs().max_elem() < 1.0) {
        PieHold::Keep
    } else {
        PieHold::Move
    }
}

/// How far the center of the pie is from the center of its window, in points of the window, when
/// the window is not where it was asked to be.
///
/// The pie is asked for at `target` (the top-left corner that centers the window on the pointer),
/// but a window manager keeps a window on the screen: near an edge or a corner it puts the window
/// somewhere else (see [`pie_hold`], which gives up after a moment). The ring used to be drawn at
/// the center of the window, so there it was drawn off the pointer while the tap, which measures
/// from the pointer, still hit-tested around the pointer: the middle of the pie seemed to slide
/// toward the center of the screen the nearer the pointer got to a corner. The ring is now drawn
/// at the point the pointer is on, wherever the window really is: `target` minus where it is
/// (`outer_min`, `None` when unknown), which is the vector to add to the center of the window.
///
/// Nothing (zero) is returned when the position is unknown, when it is within one point of the
/// right one (the window sits on whole physical pixels, and a ring moved by a fraction of a point
/// would be resampled and look soft) and when the two are more than half a window plus `slack`
/// apart: that is a position read back before the window moved (or one of another screen), not a
/// window pushed in from an edge, and the ring is better at the center of what is on screen.
pub fn pie_center_shift(
    target: Pos2,
    outer_min: Option<Pos2>,
    window: f32,
    slack: f32,
) -> egui::Vec2 {
    let Some(actual) = outer_min else {
        return egui::Vec2::ZERO;
    };
    let d = target - actual;
    if d.abs().max_elem() < 1.0 || d.abs().max_elem() > window / 2.0 + slack {
        return egui::Vec2::ZERO;
    }
    d
}

/// The pie window while *Show slice names* is on: as big as the display `display` (cursor points),
/// in window points (`scale` converts one to the other), so that the pill of the names can sit at
/// the bottom center of the display and not follow the ring around.
///
/// `None` when the display would not even hold the pie window of `min_side` (a display that small
/// or a bad reading of it): the caller then keeps the small square window. The size is whole
/// points, so the window sits on whole pixels like the small one does.
pub fn names_window_size(display: egui::Rect, scale: f32, min_side: f32) -> Option<egui::Vec2> {
    let size = egui::vec2(
        (display.width() * scale).round(),
        (display.height() * scale).round(),
    );
    (size.x.is_finite() && size.y.is_finite() && size.x >= min_side && size.y >= min_side)
        .then_some(size)
}

/// The display containing `p` (global coordinates, same space as the cursor).
pub fn screen_rect_at(p: Pos2) -> Option<egui::Rect> {
    pick_display(&platform::display_rects(), p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn d(x: f64, y: f64, w: f64, h: f64) -> ScreenRect {
        ScreenRect { x, y, w, h }
    }

    const HOLD: Duration = Duration::from_millis(300);

    fn hold_after(ms: u64, outer: Option<Pos2>) -> PieHold {
        pie_hold(
            egui::pos2(404.0, 295.0),
            outer,
            Duration::from_millis(ms),
            HOLD,
        )
    }

    #[test]
    fn a_window_centered_by_the_window_manager_is_moved_back() {
        // seen in the debug log on KWin: asked for 404,295, the window mapped at 860,417
        assert_eq!(
            hold_after(15, Some(egui::pos2(860.0, 417.0))),
            PieHold::Move
        );
    }

    #[test]
    fn a_window_in_place_is_left_alone() {
        assert_eq!(
            hold_after(15, Some(egui::pos2(404.0, 295.0))),
            PieHold::Keep
        );
        // less than a point off is the same place (physical pixel rounding)
        assert_eq!(
            hold_after(15, Some(egui::pos2(404.4, 294.6))),
            PieHold::Keep
        );
        assert_eq!(
            hold_after(15, Some(egui::pos2(405.0, 295.0))),
            PieHold::Move
        );
    }

    #[test]
    fn an_unknown_position_asks_again() {
        assert_eq!(hold_after(0, None), PieHold::Move);
    }

    #[test]
    fn the_watch_ends_even_if_the_window_stays_elsewhere() {
        // pinned to the edge of the screen by the window manager: do not fight it forever
        assert_eq!(hold_after(300, Some(egui::pos2(0.0, 312.0))), PieHold::Move);
        assert_eq!(
            hold_after(301, Some(egui::pos2(0.0, 312.0))),
            PieHold::Release
        );
        assert_eq!(hold_after(301, None), PieHold::Release);
    }

    #[test]
    fn a_window_in_place_does_not_move_the_ring() {
        let t = egui::pos2(404.0, 295.0);
        assert_eq!(pie_center_shift(t, Some(t), 386.0, 64.0), egui::Vec2::ZERO);
        // under a point is the same place
        assert_eq!(
            pie_center_shift(t, Some(egui::pos2(404.4, 294.6)), 386.0, 64.0),
            egui::Vec2::ZERO
        );
        assert_eq!(pie_center_shift(t, None, 386.0, 64.0), egui::Vec2::ZERO);
    }

    #[test]
    fn a_window_pushed_in_from_the_edge_moves_the_ring_back_to_the_pointer() {
        // asked for 100 points left of the screen: the window manager put it at 0, so the ring,
        // which is 100 points left of the center of the window, is drawn there
        let t = egui::pos2(-100.0, 300.0);
        let s = pie_center_shift(t, Some(egui::pos2(0.0, 300.0)), 386.0, 64.0);
        assert_eq!(s, egui::vec2(-100.0, 0.0));
        // the corner: both ways at once
        let s = pie_center_shift(
            egui::pos2(-150.0, -60.0),
            Some(egui::pos2(0.0, 24.0)),
            386.0,
            64.0,
        );
        assert_eq!(s, egui::vec2(-150.0, -84.0));
    }

    #[test]
    fn a_position_far_from_the_request_is_not_an_edge_and_is_ignored() {
        // KWin put the window at the center of the screen: the pie is moved back, not drawn off it
        let s = pie_center_shift(
            egui::pos2(404.0, 295.0),
            Some(egui::pos2(860.0, 417.0)),
            386.0,
            64.0,
        );
        assert_eq!(s, egui::Vec2::ZERO);
    }

    #[test]
    fn the_display_under_the_point_wins() {
        let v = [d(0.0, 0.0, 1440.0, 900.0), d(1440.0, 0.0, 1920.0, 1080.0)];
        let r = pick_display(&v, egui::pos2(2000.0, 500.0)).unwrap();
        assert_eq!(r.min, egui::pos2(1440.0, 0.0));
        assert_eq!(r.size(), egui::vec2(1920.0, 1080.0));
    }

    #[test]
    fn a_point_outside_every_display_uses_the_first() {
        let v = [d(0.0, 0.0, 100.0, 100.0), d(100.0, 0.0, 100.0, 100.0)];
        assert_eq!(
            pick_display(&v, egui::pos2(-50.0, -50.0)).unwrap().min,
            egui::pos2(0.0, 0.0)
        );
        assert!(pick_display(&[], egui::pos2(0.0, 0.0)).is_none());
    }

    #[test]
    fn displays_to_the_left_have_negative_coordinates() {
        let v = [d(0.0, 0.0, 1000.0, 800.0), d(-1280.0, 0.0, 1280.0, 1024.0)];
        let r = pick_display(&v, egui::pos2(-10.0, 10.0)).unwrap();
        assert_eq!(r.min.x, -1280.0);
    }

    #[test]
    fn the_window_of_the_names_is_the_display_or_nothing() {
        let d = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1920.0, 1080.0));
        assert_eq!(
            names_window_size(d, 1.0, 300.0),
            Some(egui::vec2(1920.0, 1080.0))
        );
        assert_eq!(
            names_window_size(d, 1.5, 300.0),
            Some(egui::vec2(2880.0, 1620.0))
        );
        // a display that cannot hold the pie, or a reading that makes no sense: the small window
        let tiny = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 200.0));
        assert_eq!(names_window_size(tiny, 1.0, 300.0), None);
        assert_eq!(names_window_size(d, f32::NAN, 300.0), None);
    }
}
