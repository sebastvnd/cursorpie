//! The animation of the lock (see [`tap::touch::lock`](crate::tap)): locking turns the pie into a
//! padlock that snaps shut, unlocking opens the padlock and turns it back into the pie.
//!
//! It is drawn in the window of the pie, where the pie was, with plain shapes (a ring of slices, a
//! rounded body, a shackle): no image and no font are needed, and the colors are the pie's own (the
//! slices of the appearance, the accent the person chose). The timeline is a pure function of the
//! time ([`scene`]) so it can be tested; [`paint`] only turns a [`Scene`] into shapes.
//!
//! ```text
//! locking     pie ring ──► ring shrinks and spins, slices close up ──► padlock pops in, shackle
//!             comes down, a small pulse when it snaps shut ──► fades away (a pill follows)
//! unlocking   the same, backwards: the shackle lifts, the padlock opens into the pie ring
//! ```

use super::{genz, glass};
use crate::config::cfg;
use crate::ui::draw::{
    arc_points, dark, ease_out_back, genz_colors, on_accent, pie_colors_for, rgb,
};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Shape, Stroke, Vec2};
use std::f32::consts::{PI, TAU};
use std::time::Instant;

/// Seconds the whole animation takes, from the first frame to the last.
pub const LOCK_SECS: f32 = 1.1;
/// The pie and the padlock change into each other during this much of it.
const MORPH_END: f32 = 0.72;
/// From here the picture fades away.
const FADE_START: f32 = 0.86;
/// How many slices the ring that stands for the pie has.
const SLICES: usize = 6;

/// A lock animation that is running.
#[derive(Clone, Copy, Debug)]
pub struct LockAnim {
    /// Locking (the pie becomes a padlock) or unlocking (the padlock becomes the pie).
    pub locked: bool,
    pub started: Instant,
}

impl LockAnim {
    pub fn new(locked: bool) -> Self {
        Self {
            locked,
            started: Instant::now(),
        }
    }
}

/// What is on the screen at one moment, every number from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scene {
    /// How much of the pie ring is there (1 = the whole pie).
    pub ring: f32,
    /// How much of the padlock is there (1 = full size).
    pub body: f32,
    /// How far the shackle is down (1 = closed, 0 = lifted open).
    pub close: f32,
    /// Opacity of everything.
    pub alpha: f32,
    /// The little pulse when the lock snaps shut (locking only).
    pub bump: f32,
}

fn smooth(x: f32, a: f32, b: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The picture `t` seconds after the animation started.
pub fn scene(t: f32, locked: bool) -> Scene {
    let morph = (t / MORPH_END).clamp(0.0, 1.0);
    // 0 = the pie, 1 = the closed padlock; unlocking runs the same way backwards
    let p = if locked { morph } else { 1.0 - morph };
    let fade_in = if locked { 1.0 } else { smooth(t, 0.0, 0.08) };
    let fade_out = 1.0 - smooth(t, FADE_START, LOCK_SECS);
    let bump = if locked {
        (1.0 - ((t - 0.69) / 0.09).abs()).max(0.0)
    } else {
        0.0
    };
    Scene {
        ring: 1.0 - smooth(p, 0.0, 0.55),
        body: smooth(p, 0.30, 0.75),
        close: smooth(p, 0.62, 0.95),
        alpha: fade_in * fade_out,
        bump,
    }
}

/// The colors of the picture.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// The slices of the pie ring.
    pub slice: Color32,
    /// The padlock.
    pub accent: Color32,
    /// The keyhole.
    pub ink: Color32,
}

impl Look {
    /// The colors of the pie the settings ask for (the same choice `pie_ui` makes).
    pub fn current() -> Self {
        let c = cfg();
        let glass = glass::Glass::of(&c);
        let genz = genz::of(&c);
        let smoked = glass.is_some() || (dark() && !genz);
        let colors = if genz {
            genz_colors()
        } else {
            pie_colors_for(smoked)
        };
        let accent = crate::ui::hud::accent_of(&c);
        Self {
            slice: rgb(colors.slice),
            accent,
            ink: on_accent(accent),
        }
    }
}

/// Draws `sc` centered on `c`. `outer` and `inner` are the radii of the pie ring (`pie_outer` and
/// `pie_inner` of the pie that was there).
pub fn paint(painter: &egui::Painter, c: Pos2, outer: f32, inner: f32, sc: Scene, look: &Look) {
    let s = outer * 0.5; // the unit of the padlock
    let a = sc.alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }

    // the pie: a ring of slices that shrinks, turns and closes up
    let ring = sc.ring.clamp(0.0, 1.0);
    if ring > 0.01 {
        let r_out = lerp(s * 1.05, outer, ring);
        let r_in = lerp(s * 0.80, inner, ring).min(r_out - 1.0);
        let mid = (r_out + r_in) / 2.0;
        let width = (r_out - r_in).max(1.0);
        let gap = lerp(0.05, 0.10, ring);
        let spin = (1.0 - ring) * 1.3;
        let k = (ring * 1.4).min(1.0) * a;
        let col = look.slice.gamma_multiply(k);
        // a faint rim, so the ring reads on a light page too
        let rim = Color32::from_black_alpha((38.0 * k) as u8);
        painter.circle_stroke(c, r_out, Stroke::new(1.0, rim));
        painter.circle_stroke(c, r_in, Stroke::new(1.0, rim));
        for i in 0..SLICES {
            let a0 = spin + TAU * i as f32 / SLICES as f32 + gap / 2.0 - PI / 2.0;
            let a1 = spin + TAU * (i + 1) as f32 / SLICES as f32 - gap / 2.0 - PI / 2.0;
            painter.add(Shape::line(
                arc_points(c, mid, a0, a1),
                Stroke::new(width, col),
            ));
        }
    }

    // the padlock: pops in, the shackle comes down (or lifts), a pulse when it snaps shut
    let body = sc.body.clamp(0.0, 1.0);
    if body > 0.01 {
        let pop = ease_out_back(body).max(0.0);
        let k = s * pop * (1.0 + 0.07 * sc.bump);
        let alpha = body * a;
        let col = look.accent.gamma_multiply(alpha);
        let ink = look.ink.gamma_multiply(alpha);
        // a soft disc behind it, so the padlock reads on any page
        painter.circle_filled(
            c,
            s * 1.18 * pop,
            Color32::from_black_alpha((46.0 * alpha) as u8),
        );
        let body_c = c + Vec2::new(0.0, 0.28 * k);
        let body_top = body_c.y - 0.5 * k;
        // the shackle first: its legs go into the body, which is drawn over them
        let sw = 0.17 * k;
        let leg = 0.40 * k;
        let lift = (1.0 - sc.close.clamp(0.0, 1.0)) * 0.34 * k;
        let arc_c = Pos2::new(c.x, body_top - 0.30 * k - lift);
        let right_bottom = Pos2::new(c.x + leg, body_top + 0.15 * k - lift);
        let mut pts = vec![
            Pos2::new(c.x - leg, body_top + 0.15 * k),
            Pos2::new(c.x - leg, arc_c.y),
        ];
        pts.extend(arc_points(arc_c, leg, PI, TAU));
        pts.push(right_bottom);
        painter.add(Shape::line(pts, Stroke::new(sw, col)));
        painter.circle_filled(right_bottom, sw / 2.0, col);
        // the body, and its keyhole
        let round = CornerRadius::same((0.22 * k).round().clamp(0.0, 255.0) as u8);
        painter.rect_filled(
            Rect::from_center_size(body_c, Vec2::new(1.30 * k, 1.00 * k)),
            round,
            col,
        );
        let hole = body_c + Vec2::new(0.0, -0.10 * k);
        painter.circle_filled(hole, 0.13 * k, ink);
        painter.rect_filled(
            Rect::from_center_size(
                hole + Vec2::new(0.0, 0.17 * k),
                Vec2::new(0.09 * k, 0.26 * k),
            ),
            CornerRadius::same((0.03 * k).round().clamp(0.0, 255.0) as u8),
            ink,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn locking_starts_as_the_pie_and_ends_as_a_closed_padlock() {
        let start = scene(0.0, true);
        assert!(near(start.ring, 1.0) && near(start.body, 0.0) && near(start.alpha, 1.0));
        let end = scene(MORPH_END, true);
        assert!(near(end.ring, 0.0) && near(end.body, 1.0) && near(end.close, 1.0));
        assert!(
            near(end.alpha, 1.0),
            "still fully shown when the shackle is down"
        );
    }

    #[test]
    fn unlocking_is_the_same_the_other_way_round() {
        let start = scene(0.0, false);
        assert!(near(start.body, 1.0) && near(start.close, 1.0) && near(start.ring, 0.0));
        assert!(
            near(start.alpha, 0.0) && near(scene(0.08, false).alpha, 1.0),
            "it fades in"
        );
        let end = scene(MORPH_END, false);
        assert!(near(end.ring, 1.0) && near(end.body, 0.0) && near(end.close, 0.0));
    }

    #[test]
    fn the_picture_fades_out_completely_and_the_pulse_is_for_locking_only() {
        assert!(near(scene(LOCK_SECS, true).alpha, 0.0));
        assert!(near(scene(LOCK_SECS, false).alpha, 0.0));
        assert!(scene(0.69, true).bump > 0.9);
        assert!(near(scene(0.69, false).bump, 0.0));
    }

    #[test]
    fn every_number_stays_between_zero_and_one() {
        for locked in [true, false] {
            for i in 0..=120 {
                let s = scene(i as f32 / 100.0, locked);
                for v in [s.ring, s.body, s.close, s.alpha, s.bump] {
                    assert!((0.0..=1.0).contains(&v), "{locked} {i}: {s:?}");
                }
            }
        }
    }
}
