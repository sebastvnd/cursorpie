//! The window of the virtual pointer: a small arrow, click-through and always on top, that stands
//! where the virtual pointer is. It is only a picture: the pointer itself is
//! [`platform::pointer_pos`](crate::platform::pointer_pos), which the pie, the trackpad source and
//! the lookup of the app under the pointer all read.

use eframe::egui::{Color32, Pos2, Shape, Stroke, Ui, ViewportBuilder, ViewportId, pos2, vec2};

/// The window's side, in points.
pub const SIZE: f32 = 28.0;
/// Where the tip of the arrow is inside the window: the window is a little bigger than the arrow.
pub const TIP: f32 = 4.0;

/// The window's id.
pub fn id() -> ViewportId {
    ViewportId::from_hash_of("cursorpie-virtual-pointer")
}

/// Where the window goes so that the tip of the arrow is at the pointer `(x, y)`. The window sits on
/// whole points: a window put at a fraction of a point shimmers as the system rounds it again and
/// again, and the arrow looks like it trembles.
pub fn window_pos(x: f64, y: f64) -> Pos2 {
    pos2((x as f32).round() - TIP, (y as f32).round() - TIP)
}

/// The window, put at `at` to begin with. It never takes the pointer, the keyboard or a place in the
/// taskbar.
pub fn builder(at: (f64, f64)) -> ViewportBuilder {
    let mut b = ViewportBuilder::default()
        .with_title("Virtual pointer")
        .with_inner_size(vec2(SIZE, SIZE))
        .with_position(window_pos(at.0, at.1))
        .with_decorations(false)
        .with_transparent(true)
        .with_has_shadow(false)
        .with_resizable(false)
        .with_mouse_passthrough(true)
        .with_always_on_top()
        .with_active(false);
    if crate::platform::OS == crate::platform::Os::Windows {
        b = b.with_taskbar(false);
    }
    if crate::platform::OS == crate::platform::Os::Linux {
        // CursorPie's own windows are never "the app under the pointer"
        b = b.with_app_id("cursorpie");
    }
    b
}

/// Draws the arrow.
pub fn show(ui: &mut Ui) {
    let p = ui.painter();
    let o = vec2(TIP, TIP);
    let at = |x: f32, y: f32| pos2(x, y) + o;
    // the tail first, white under black, so the arrow has an outline
    p.line_segment(
        [at(5.0, 13.0), at(9.2, 20.5)],
        Stroke::new(5.6, Color32::WHITE),
    );
    p.line_segment(
        [at(5.0, 13.0), at(9.2, 20.5)],
        Stroke::new(3.0, Color32::BLACK),
    );
    p.add(Shape::convex_polygon(
        vec![at(0.0, 0.0), at(0.0, 17.0), at(12.0, 12.2)],
        Color32::BLACK,
        Stroke::new(1.6, Color32::WHITE),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    #[test]
    fn the_tip_of_the_arrow_is_at_the_pointer() {
        let w = window_pos(500.0, 300.0);
        assert_eq!(w + vec2(TIP, TIP), pos2(500.0, 300.0));
    }

    #[test]
    fn the_window_stays_on_whole_points() {
        let w = window_pos(500.4, 300.6);
        assert_eq!(w + vec2(TIP, TIP), pos2(500.0, 301.0));
        assert_eq!(window_pos(10.2, 10.3), window_pos(10.0, 10.0));
    }

    #[test]
    fn the_arrow_paints() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), show);
        out.textures_delta.clear();
    }
}
