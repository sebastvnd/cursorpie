//! Trail ("web") mode: a ring that was left to open a submenu shrinks to a small node. The nodes sit
//! on an arc around the ring on screen, in the order the rings were opened: the ring just left is
//! the nearest, on a bold thread to the center of the ring on screen, and the older ones follow
//! round the arc, each smaller and paler than the one before, on a thread that carries small
//! arrows toward the ring on screen. So the way you came reads like a path, with the root at its far
//! end. A node shows what the center of its ring showed (an app icon, the icon of a mode or
//! submenu, or the CursorPie mark). Moving the pointer onto one returns to that ring (the decision
//! is the tap's, see `tap::trigger`; where the nodes go is `geometry::trail_layout`, which both
//! use; this only draws them).

use crate::ui::draw::*;
use eframe::egui::{self, Color32, Pos2, Shape, Stroke, Vec2};

/// How the trail is painted.
pub(super) struct Look {
    pub accent: Color32,
    pub colors: PieColors,
    /// 0 to 1: the fade of the whole pie (opening and closing)
    pub fade: f32,
    /// Whether the thread (and its arrows) is drawn; off: only the nodes
    pub thread: bool,
}

/// One node: where it is now (it glides to its place), how big, and what it shows.
pub(super) struct Node<'a> {
    pub at: Pos2,
    pub r: f32,
    /// The ring just left: its node is drawn with the accent, the others quieter.
    pub near: bool,
    /// The icon of the app that ring belongs to, when it has one
    pub app_tex: Option<&'a egui::TextureHandle>,
    /// Whether that icon is drawn in its own colors
    pub app_icon_color: bool,
    /// The icon of the active mode, or of the submenu slice that ring was opened from
    pub icon: &'a str,
    pub icon_tex: Option<&'a egui::TextureHandle>,
}

/// How many text sizes a node's icon is cut at per doubling (see [`glyph_ladder`]).
const GLYPH_STEPS_PER_OCTAVE: f32 = 8.0;

/// `size` snapped to a short ladder of sizes (8 per doubling, about 9% apart, so the step is
/// not seen).
///
/// A node grows as it glides to its place, so the size of its icon is a continuous value. egui
/// cuts a new set of glyphs into its font atlas for every text size it is asked for and never
/// gives one back, so the icon of a node (an emoji, a letter or a glyph of the icon font, which
/// are all text) is laid out at a size from this ladder: a few sizes over the whole range of a
/// node, instead of one per frame of every glide. A picture is scaled by the painter for free and
/// keeps the exact size.
pub(super) fn glyph_ladder(size: f32) -> f32 {
    if !size.is_finite() || size < 1.0 {
        return 1.0;
    }
    ((size.log2() * GLYPH_STEPS_PER_OCTAVE).round() / GLYPH_STEPS_PER_OCTAVE).exp2()
}

/// How much of its strength a node and its thread keep: the nearest has all of it, each older ring
/// a little less.
fn age(slot: usize) -> f32 {
    (1.0 - 0.13 * slot as f32).max(0.45)
}

/// A small arrowhead at `at`, pointing along `dir` (a unit vector).
fn chevron(painter: &egui::Painter, at: Pos2, dir: Vec2, size: f32, stroke: Stroke) {
    let side = Vec2::new(-dir.y, dir.x);
    let tip = at + dir * size * 0.6;
    let back = at - dir * size * 0.4;
    painter.add(Shape::line(
        vec![back + side * size * 0.7, tip, back - side * size * 0.7],
        stroke,
    ));
}

/// Paints the trail: `nodes` come nearest first. A bold thread runs from `child`, the center of the
/// ring on screen, to the nearest node, and a thinner one on from node to node, with an arrow in the
/// middle of each pointing toward the ring on screen, the way the path was walked. Call it before
/// the ring itself, so that one is in front.
pub(super) fn paint(painter: &egui::Painter, child: Pos2, nodes: &[Node], look: &Look) {
    let Look {
        accent,
        colors,
        fade,
        thread,
    } = *look;

    // the thread: dark and thick enough to read on any page, with a pale edge for a dark one
    let line = |from: Pos2, to: Pos2, w: f32, a: f32| {
        painter.line_segment(
            [from, to],
            Stroke::new(w + 2.0, colors.core.gamma_multiply(0.55 * fade)),
        );
        let stroke = Stroke::new(w, colors.ink.gamma_multiply(a * fade));
        painter.line_segment([from, to], stroke);
        // an arrow in the middle, toward `to`
        let d = to - from;
        if d.length() > 24.0 {
            chevron(painter, from + d * 0.5, d.normalized(), 6.0, stroke);
        }
    };
    if thread {
        if let Some(first) = nodes.first() {
            line(first.at, child, 3.0, 0.9);
        }
        for (slot, w) in nodes.windows(2).enumerate() {
            // from the older node to the newer one
            line(w[1].at, w[0].at, 2.0, 0.65 * age(slot + 1));
        }
    }

    // the nodes, the oldest first so the nearest is in front
    for (slot, n) in nodes.iter().enumerate().rev() {
        let a = age(slot);
        let (edge, ink) = if n.near {
            (accent.gamma_multiply(fade), 0.75)
        } else {
            (colors.ink.gamma_multiply(0.6 * a * fade), 0.6 * a)
        };
        painter.circle_filled(
            n.at,
            n.r,
            colors.core.gamma_multiply((0.6 + 0.37 * a) * fade),
        );
        painter.circle_stroke(n.at, n.r, Stroke::new(2.0, edge));
        if let Some(t) = n.app_tex {
            let tint = if n.app_icon_color {
                Color32::WHITE.gamma_multiply(a * fade)
            } else {
                colors.ink.gamma_multiply(0.85 * a * fade)
            };
            draw_texture(painter, t, n.at, n.r * 1.3, tint);
        } else if n.icon.trim().is_empty() {
            // nothing to show: the CursorPie mark, as the center of a pie does
            let rect = egui::Rect::from_center_size(n.at, Vec2::splat(n.r * 1.2));
            crate::ui::settings::brand::paint(
                painter,
                crate::ui::settings::brand::Art::Mark,
                rect,
                colors.ink.gamma_multiply(ink * fade),
            );
        } else {
            let size = if n.icon_tex.is_some() {
                n.r * 1.1
            } else {
                // text: cut at a size of the ladder, and the font atlas is rebuilt when the UI
                // sleeps (see `App::release_font_atlas`), as for the zoomed text of Settings
                crate::ui::settings::note_glyph_sizes();
                glyph_ladder(n.r * 1.1)
            };
            draw_icon(
                painter,
                n.icon_tex,
                n.icon,
                n.at,
                size,
                colors.ink.gamma_multiply(ink * fade),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glide_cuts_a_handful_of_text_sizes_not_one_per_frame() {
        // a node of the default pie grows from 55% to 100% of its size over about 20 frames,
        // and the oldest one is a third of the nearest: sweep the whole range in small steps
        let mut sizes: Vec<u32> = (0..2000)
            .map(|i| 8.0 + 22.0 * i as f32 / 2000.0)
            .map(|v| (glyph_ladder(v) * 100.0).round() as u32)
            .collect();
        sizes.sort_unstable();
        sizes.dedup();
        assert!(sizes.len() <= 20, "{} sizes: {sizes:?}", sizes.len());
    }

    #[test]
    fn the_ladder_stays_close_to_the_size_and_is_stable() {
        for v in [6.0_f32, 9.3, 14.0, 21.7, 40.0] {
            let g = glyph_ladder(v);
            assert!((g / v - 1.0).abs() < 0.05, "{v} -> {g}");
            assert!((glyph_ladder(g) - g).abs() < 1e-3, "a step maps to itself");
        }
        assert_eq!(glyph_ladder(f32::NAN), 1.0);
        assert_eq!(glyph_ladder(-3.0), 1.0);
    }
}
