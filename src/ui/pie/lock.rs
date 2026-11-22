//! The animation of the lock (see [`tap::touch::lock`](crate::tap)): locking turns the pie into a
//! padlock that snaps shut, unlocking opens the padlock and the pie comes out of it.
//!
//! **Locking** keeps the pie that was open: its own slices, icons and material (solid, glass, Gen Z)
//! are drawn by `pie_ui` as always, only squeezed, turned and faded by the numbers of [`scene`],
//! while [`paint_padlock`] draws the padlock over its middle. Nothing of the pie is swapped for a
//! plain stand-in, so the slices and their icons do not vanish into bare shapes half way.
//!
//! **Unlocking** with a trigger that is still held opens a real pie at once (see
//! [`reveal_scene`]): the padlock sits in the middle, its shackle lifts and it fades while the pie
//! blooms around it, so the person never has to trigger the pie again.
//!
//! [`paint`] (plain ring + padlock) is only the fallback for a window with no pie to start from,
//! for example an unlock that could not open a pie. The timeline is a pure function of the time
//! ([`scene`]) so it can be tested; the paint functions only turn a [`Scene`] into shapes.
//!
//! ```text
//! locking     pie (slices + icons) ──► icons fade, ring squeezes and turns, slices close up ──►
//!             padlock grows in, shackle comes down, a small pulse when it snaps shut ──► fades
//!             away (a pill follows)
//! unlocking   padlock in the middle ──► shackle lifts, padlock fades and grows a little while
//!             the pie blooms around it ──► the pie is open and live
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
pub const LOCK_SECS: f32 = 1.15;
/// The pie and the padlock change into each other during this much of it.
const MORPH_END: f32 = 0.78;
/// From here the picture fades away.
const FADE_START: f32 = 0.92;
/// When the shackle has just snapped shut and the padlock gives its little pulse.
const BUMP_AT: f32 = 0.74;
/// Seconds the padlock stays over a pie that opened out of an unlock.
pub const REVEAL_SECS: f32 = 0.55;
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

/// Smootherstep: zero speed and zero acceleration at both ends, so each part of the animation
/// starts and stops without a visible kink (the plain smoothstep has a speed jump there).
pub fn smooth(x: f32, a: f32, b: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    // float rounding can land a hair outside 0..1 (1.0000002); `1.0 - smooth(..)` would then be
    // slightly negative, and egui's `gamma_multiply` panics on a negative factor
    (t * t * t * (t * (t * 6.0 - 15.0) + 10.0)).clamp(0.0, 1.0)
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
        (1.0 - ((t - BUMP_AT) / 0.10).abs()).max(0.0)
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

/// The padlock over a pie that has just opened out of an unlock, `t` seconds after it began: the
/// shackle lifts first, then the padlock fades while it grows a little (the caller scales it with
/// [`reveal_grow`]). `ring` and `bump` are not used.
pub fn reveal_scene(t: f32) -> Scene {
    let k = (t / REVEAL_SECS).clamp(0.0, 1.0);
    Scene {
        ring: 1.0,
        body: 1.0,
        close: 1.0 - smooth(k, 0.0, 0.55),
        alpha: 1.0 - smooth(k, 0.35, 1.0),
        bump: 0.0,
    }
}

/// How much bigger than its start the padlock of [`reveal_scene`] is `t` seconds in (1.0 to 1.3).
pub fn reveal_grow(t: f32) -> f32 {
    1.0 + 0.3 * smooth(t / REVEAL_SECS, 0.2, 1.0)
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

    paint_padlock(painter, c, outer, sc, look);
}

/// Draws the padlock of `sc` centered on `c`; `outer` is the radius of the pie it belongs to (the
/// padlock is half that wide). It is drawn over whatever is there, so it can sit on the real pie.
pub fn paint_padlock(painter: &egui::Painter, c: Pos2, outer: f32, sc: Scene, look: &Look) {
    let s = outer * 0.5; // the unit of the padlock
    let a = sc.alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
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
        assert!(scene(BUMP_AT, true).bump > 0.9);
        assert!(near(scene(BUMP_AT, false).bump, 0.0));
    }

    #[test]
    fn the_padlock_over_an_unlocked_pie_opens_then_fades_and_grows() {
        let start = reveal_scene(0.0);
        assert!(near(start.close, 1.0) && near(start.alpha, 1.0) && near(start.body, 1.0));
        let end = reveal_scene(REVEAL_SECS);
        assert!(near(end.close, 0.0) && near(end.alpha, 0.0));
        assert!(near(reveal_grow(0.0), 1.0) && reveal_grow(REVEAL_SECS) > 1.2);
        for i in 0..=70 {
            let s = reveal_scene(i as f32 / 100.0);
            for v in [s.ring, s.body, s.close, s.alpha, s.bump] {
                assert!((0.0..=1.0).contains(&v), "{i}: {s:?}");
            }
        }
    }

    #[test]
    fn the_curve_is_flat_at_both_ends_and_never_leaves_zero_to_one() {
        assert!(near(smooth(0.0, 0.0, 1.0), 0.0) && near(smooth(1.0, 0.0, 1.0), 1.0));
        assert!(near(smooth(0.5, 0.0, 1.0), 0.5));
        // flat at the ends: a tenth of the way in has moved far less than a tenth
        assert!(smooth(0.1, 0.0, 1.0) < 0.01);
        let mut last = 0.0;
        for i in 0..=100 {
            let v = smooth(i as f32 / 100.0, 0.0, 1.0);
            assert!((0.0..=1.0).contains(&v) && v >= last);
            last = v;
        }
    }

    #[test]
    fn the_curve_never_rounds_outside_zero_to_one() {
        for a in [0.0_f32, 0.35, 0.62, 0.92] {
            for b in [0.4_f32, 0.55, 1.0, 1.15] {
                for i in 0..=2000 {
                    let v = smooth(i as f32 / 1000.0, a, b);
                    assert!((0.0..=1.0).contains(&v), "{a} {b} {i}: {v}");
                    assert!((0.0..=1.0).contains(&(1.0 - v)));
                }
            }
        }
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
