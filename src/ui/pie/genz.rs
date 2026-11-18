//! The "Gen Z" look of the pie: the pie of the website. White slices with thick near-black
//! outlines, a hard shadow (no blur) that falls down and to the right, and the accent on the slice
//! under the pointer. It is the same in the light and the dark appearance: the outlines, not the
//! fill, are what set it off the page.
//!
//! The functions mirror [`super::glass`] (`slice`, `core`), so the pie, its badges and the preview
//! in Settings pick a material the same way and share the drawing.

use crate::config::{Config, PieStyle};
use crate::ui::draw::{Bend, genz_colors, rgb, ring_slice, ring_slice_outline};
use eframe::egui::{Color32, Painter, Pos2, Shape, Stroke, Vec2};

/// Width of every outline, in points (2.4 on the 340-pixel pie of the website).
const STROKE: f32 = 2.2;
/// How far the hard shadow falls (right, down), in points, while the slice is at rest.
const OFFSET: Vec2 = Vec2::new(3.0, 4.0);
/// How much farther it falls under the slice the pointer is on (0.5 = half as far again).
const LIFT: f32 = 0.5;

/// Whether `cfg` draws the pie in this style.
pub fn of(cfg: &Config) -> bool {
    cfg.pie_style == PieStyle::GenZ
}

/// The ink of the outlines and of the shadow, faded by `fade`.
fn ink(fade: f32) -> Color32 {
    genz_colors().ink.gamma_multiply(fade)
}

fn outline(fade: f32) -> Stroke {
    Stroke::new(STROKE, ink(fade))
}

/// Where the shadow of a shape lies from the shape; `hover` is 0 to 1.
fn shadow_offset(hover: f32) -> Vec2 {
    OFFSET * (1.0 + LIFT * hover)
}

/// One slice: its hard shadow, an opaque body of `fill` and its outline. `hover` (0 to 1) lifts it
/// off the page, `fade` is the opacity of the whole. A lone slice is a full ring, whose outline
/// would draw the seam as a line, so it gets none (the ring of the pie outlines it instead). The
/// slice, its shadow and its outline are bent to the outline of the pie by `bend`.
#[allow(clippy::too_many_arguments)]
pub fn slice(
    painter: &Painter,
    c: Pos2,
    bend: Bend,
    (inner, outer): (f32, f32),
    (a0, a1): (f32, f32),
    fill: [u8; 3],
    hover: f32,
    fade: f32,
    lone: bool,
) {
    let at = c + shadow_offset(hover);
    painter.add(ring_slice(
        painter,
        at,
        inner,
        outer,
        a0,
        a1,
        ink(fade),
        bend,
    ));
    let body = rgb(fill).gamma_multiply(fade);
    painter.add(ring_slice(painter, c, inner, outer, a0, a1, body, bend));
    if !lone {
        painter.add(Shape::closed_line(
            bend.points(c, ring_slice_outline(c, inner, outer, a0, a1)),
            outline(fade),
        ));
    }
}

/// A round piece in the same material: the center of the pie and the badges of a slice.
pub fn core(painter: &Painter, c: Pos2, radius: f32, fade: f32, fill: Color32) {
    painter.circle_filled(c + shadow_offset(0.0), radius, ink(fade));
    painter.circle_filled(c, radius, fill.gamma_multiply(fade));
    painter.circle_stroke(c, radius, outline(fade));
}

/// The stroke of the ring around a pie made of one slice (see [`slice()`]), and none for a pie with
/// seams: the outlines of its slices are already its edge.
pub fn ring(fade: f32, lone: bool) -> (Color32, f32) {
    (ink(fade), if lone { STROKE } else { 0.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shadow_falls_farther_under_the_hovered_slice() {
        assert_eq!(shadow_offset(0.0), OFFSET);
        assert!(shadow_offset(1.0).length() > shadow_offset(0.0).length());
    }

    #[test]
    fn only_the_gen_z_style_draws_this_material() {
        let mut cfg = Config::default();
        // a fresh install is in this style
        assert!(of(&cfg));
        cfg.pie_style = PieStyle::Solid;
        assert!(!of(&cfg));
        cfg.pie_style = PieStyle::Glass;
        assert!(!of(&cfg));
        cfg.pie_style = PieStyle::GenZ;
        assert!(of(&cfg));
    }

    #[test]
    fn a_pie_with_seams_has_no_ring_of_its_own() {
        assert_eq!(ring(1.0, false).1, 0.0);
        assert_eq!(ring(1.0, true).1, STROKE);
    }
}
