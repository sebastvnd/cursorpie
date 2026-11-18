//! The "glass" look of the pie: translucent slices that catch a light from the top left, drawn the
//! way Apple's Liquid Glass looks.
//!
//! The window cannot blur what is behind it (a transparent window gives the program nothing to
//! read from the screen), so the material is painted, and painted as *smooth fields*, never as
//! steps. Each slice is one body color plus one mesh whose vertices carry a continuous light
//! value: a frost that thins out from the inner edge, a sheen that swells toward the outer rim, a
//! thin bright rim and a soft glow just inside it. The rim is not equally bright all the way
//! round: it follows one light, so it is brightest on the side facing the top left and fades to
//! almost nothing on the far side (with a weaker answering glint at the opposite corner), which
//! is what makes the ring read as a single piece of curved glass. Three numbers in the
//! configuration shape it, all in percent: how clear, how frosted and how shiny.

use crate::config::Config;
use crate::ui::draw::{Bend, arc_segments, glass_colors, ring_slice};
use eframe::egui::{self, Color32, Painter, Pos2, Shape, Vec2};
use std::f32::consts::{PI, TAU};

/// Largest distance, in points, between a drawn arc and the true circle.
const ARC_TOLERANCE: f32 = 0.05;
/// Angle (radians, y down) the light comes from: the top left.
const LIGHT: f32 = -3.0 * PI / 4.0;
/// Distances, in points, from an edge at which the mesh gets its own row or column of vertices.
/// The light changes fastest next to an edge, so that is where the vertices are packed.
const EDGE_STOPS: [f32; 6] = [0.0, 0.7, 1.6, 3.0, 5.5, 9.0];
/// Farthest apart, in points, two rows of the mesh may be.
const MAX_GAP: f32 = 6.0;

/// The three glass settings, each `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glass {
    /// How see-through the slice is (1 = almost clear).
    pub clear: f32,
    /// How frosted it is (1 = a thick milky veil).
    pub frost: f32,
    /// How bright the rim and the sheen are.
    pub shine: f32,
}

impl Glass {
    /// The glass of `cfg`, or `None` while the pie is drawn solid.
    pub fn of(cfg: &Config) -> Option<Self> {
        (cfg.pie_style == crate::config::PieStyle::Glass).then(|| Self::from_percent(cfg))
    }

    /// The glass of the glass numbers of `cfg`, whichever style `cfg` has chosen.
    pub fn from_percent(cfg: &Config) -> Self {
        let f = |v: u8| f32::from(v.min(100)) / 100.0;
        Self {
            clear: f(cfg.glass_clear),
            frost: f(cfg.glass_frost),
            shine: f(cfg.glass_shine),
        }
    }

    /// Opacity of the slice body, `0.0..=1.0`. Never fully clear, or the name would float on the
    /// page with nothing to read against.
    pub fn body_alpha(self) -> f32 {
        (1.0 - 0.85 * self.clear).clamp(0.15, 1.0)
    }

    /// Opacity of the first (innermost) step of the frost veil.
    pub fn veil_alpha(self) -> f32 {
        0.26 * self.frost
    }

    /// Opacity of the bright outline.
    pub fn rim_alpha(self) -> f32 {
        0.15 + 0.7 * self.shine
    }

    /// Opacity of the sheen along the outer rim.
    pub fn sheen_alpha(self) -> f32 {
        0.22 * self.shine
    }
}

/// The color of the frost and the highlights: the pale grey of smoked glass. Glass is smoked in
/// the light appearance too (see [`crate::ui::draw::glass_colors`]), so this never depends on it.
fn veil_color() -> [u8; 3] {
    [210, 212, 224]
}

fn with_alpha(rgb: [u8; 3], alpha: f32) -> Color32 {
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], a)
}

/// Smooth step: `0` up to `t = 0`, `1` from `t = 1`, with no kink at either end.
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Two layers of translucent paint on top of each other: `1 - (1 - a)(1 - b)`.
fn over(a: f32, b: f32) -> f32 {
    1.0 - (1.0 - a.clamp(0.0, 1.0)) * (1.0 - b.clamp(0.0, 1.0))
}

/// How much light the edge catches at `theta` (the angle around the pie's center), `0.0..=1.0`.
/// One broad lobe faces the light; a weaker one answers on the opposite side, and the two
/// flanks in between are dark. The curve is smooth all the way round.
fn lit(theta: f32) -> f32 {
    let d = theta - LIGHT;
    let lobe = (0.5 + 0.5 * (2.0 * d).cos()).powi(2);
    if d.cos() >= 0.0 { lobe } else { 0.55 * lobe }
}

/// Where a vertex of a [`field_mesh`] is, as the light needs to know it.
#[derive(Clone, Copy, Debug)]
struct Sample {
    /// Across the ring: `0` at the inner edge, `1` at the outer edge.
    u: f32,
    /// Distance, in points, to the nearest edge of the shape (a full ring has no sides, only an
    /// inner and an outer edge).
    edge: f32,
    /// Angle of the vertex around the center of the pie.
    theta: f32,
}

/// The strength of each layer of light on one slice, already scaled by hover.
#[derive(Clone, Copy, Debug)]
struct Look {
    frost: f32,
    sheen: f32,
    rim: f32,
}

impl Look {
    /// Opacity of the white (the frost, the sheen, the rim and its glow, all together) at `s`.
    /// Continuous in all three of its inputs, which is what keeps the glass free of bands.
    fn alpha(self, s: Sample) -> f32 {
        let l = lit(s.theta);
        // frost: thickest at the inner edge, thinning out smoothly
        let frost = self.frost * (1.0 - smooth(s.u));
        // sheen: swells toward the outer rim, stronger on the lit side
        let sheen = self.sheen * smooth((s.u - 0.62) / 0.38) * (0.45 + 0.55 * l);
        // rim: a thin bright line along every edge, bright only where the light hits
        let line = (self.rim * (0.28 + 0.72 * l) * (-s.edge / 0.9).exp()).min(1.0);
        // glow: the same light, spilled a few points inside the edge
        let glow = self.rim * 0.30 * (0.40 + 0.60 * l) * (-s.edge / 5.0).exp();
        over(over(frost, sheen), over(line, glow))
    }
}

/// Positions between `lo` and `hi`, packed near both ends and never more than `max_gap` apart.
/// `unit` turns the point distances of [`EDGE_STOPS`] into the units of `lo` and `hi` (`1.0` for
/// a radius, the radius itself for an angle).
fn stops(lo: f32, hi: f32, unit: f32, max_gap: f32) -> Vec<f32> {
    let span = hi - lo;
    let mut v = vec![lo, hi];
    for o in EDGE_STOPS {
        let o = o / unit.max(1.0);
        if o < span / 2.0 {
            v.push(lo + o);
            v.push(hi - o);
        }
    }
    v.sort_by(f32::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    let mut out = Vec::with_capacity(v.len() + 8);
    for w in v.windows(2) {
        out.push(w[0]);
        let n = ((w[1] - w[0]) / max_gap.max(1e-3)).ceil() as usize;
        for k in 1..n {
            out.push(w[0] + (w[1] - w[0]) * k as f32 / n as f32);
        }
    }
    out.push(v[v.len() - 1]);
    out
}

/// One column of a [`field_mesh`]: the fringe on either side, or a true angle.
#[derive(Clone, Copy)]
enum Col {
    Left,
    At(f32),
    Right,
}

/// A donut slice made of many small cells, each vertex colored by `f`, so the color changes
/// smoothly across it. The edges are anti-aliased the same way as in
/// [`crate::ui::draw::ring_slice_mesh`]: one more ring of fully transparent vertices a pixel
/// outside every edge (`feather` points), so the GPU fades the shape out instead of cutting it.
/// A full circle (`a1 - a0 >= TAU`) has no sides and no seam. An empty mesh is returned when
/// the slice has no area.
fn field_mesh(
    c: Pos2,
    (r0, r1): (f32, f32),
    (a0, a1): (f32, f32),
    feather: f32,
    f: impl Fn(Sample) -> Color32,
) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    let sweep = a1 - a0;
    if r1 - r0 < 0.25 || sweep <= 0.0 {
        return mesh;
    }
    let span = r1 - r0;
    let h = (feather / 2.0).clamp(0.0, span / 4.0);
    let closed = sweep >= TAU - 1e-3;
    let steps = arc_segments(r1, sweep, ARC_TOLERANCE);

    // rows: a fringe, the packed rows between the edges, a fringe
    let mut rows = vec![r0 - h];
    rows.extend(stops(r0 + h, r1 - h, 1.0, MAX_GAP));
    rows.push(r1 + h);

    let cols: Vec<Col> = if closed {
        (0..=steps)
            .map(|k| Col::At(a0 + sweep * k as f32 / steps as f32))
            .collect()
    } else {
        let r_ref = (r0 + r1) / 2.0;
        let ang = (h / r_ref).min(sweep / 4.0);
        let mut v = vec![Col::Left];
        v.extend(
            stops(a0 + ang, a1 - ang, r_ref, sweep / steps as f32)
                .into_iter()
                .map(Col::At),
        );
        v.push(Col::Right);
        v
    };

    for (ri, &r) in rows.iter().enumerate() {
        let fringe_row = ri == 0 || ri == rows.len() - 1;
        // the radial sides are `h` away from the edge whatever the radius: an angle of h / r
        let side = (h / r.max(1.0)).min(sweep / 4.0);
        for col in &cols {
            let (a, fringe_col) = match *col {
                Col::Left => (a0 - side, true),
                Col::Right => (a1 + side, true),
                Col::At(a) => (a, false),
            };
            let p = c + Vec2::new(a.cos(), a.sin()) * r;
            let color = if fringe_row || fringe_col {
                Color32::TRANSPARENT
            } else {
                let to_side = if closed {
                    f32::INFINITY
                } else {
                    (a - a0).min(a1 - a).max(0.0) * r
                };
                f(Sample {
                    u: ((r - r0) / span).clamp(0.0, 1.0),
                    edge: (r - r0).min(r1 - r).min(to_side).max(0.0),
                    theta: a,
                })
            };
            mesh.colored_vertex(p, color);
        }
    }
    let n = cols.len() as u32;
    for row in 0..rows.len() as u32 - 1 {
        for col in 0..n - 1 {
            let i = row * n + col;
            let below = i + n;
            mesh.add_triangle(i, i + 1, below);
            mesh.add_triangle(i + 1, below + 1, below);
        }
    }
    mesh
}

/// One physical pixel of `painter`'s display, in points.
fn pixel(painter: &Painter) -> f32 {
    1.0 / painter.ctx().pixels_per_point().max(1.0)
}

/// Paints one slice of glass. `base` is its color (the rest color blended toward the accent), `h`
/// how hovered it is and `lf` how far its entrance went (both `0.0..=1.0`). A `lone` slice is a
/// full ring; it is recognised by its sweep, so the flag only documents the call. The slice is bent
/// to the outline of the pie by `bend`.
#[allow(clippy::too_many_arguments)]
pub fn slice(
    painter: &Painter,
    center: Pos2,
    bend: Bend,
    (inner, outer): (f32, f32),
    (s0, s1): (f32, f32),
    base: [u8; 3],
    h: f32,
    lf: f32,
    glass: Glass,
    _lone: bool,
) {
    // a hovered slice becomes more solid, so the accent reads
    let body = glass.body_alpha() + (1.0 - glass.body_alpha()) * 0.55 * h;
    painter.add(ring_slice(
        painter,
        center,
        inner,
        outer,
        s0,
        s1,
        with_alpha(base, body * lf),
        bend,
    ));

    let feather = pixel(painter);

    // depth: a faint dark edge, strongest on the side the light does not reach
    let dark = field_mesh(center, (inner, outer), (s0, s1), feather, |s| {
        let shade = 1.0 - lit(s.theta);
        let a = 0.13 * lf * (0.4 + 0.6 * shade) * (-s.edge / 0.9).exp();
        Color32::from_black_alpha((a * 255.0).round() as u8)
    });
    let dark = bend.mesh(center, dark);
    if !dark.is_empty() {
        painter.add(Shape::mesh(dark));
    }

    // light: frost, sheen, rim and glow in one mesh, so there is no seam between them
    let look = Look {
        frost: glass.veil_alpha() * (1.0 - 0.5 * h),
        sheen: glass.sheen_alpha(),
        rim: (glass.rim_alpha() * (1.0 + 0.3 * h)).min(1.0),
    };
    let veil = veil_color();
    let light = field_mesh(center, (inner, outer), (s0, s1), feather, |s| {
        with_alpha(veil, look.alpha(s) * lf)
    });
    let light = bend.mesh(center, light);
    if !light.is_empty() {
        painter.add(Shape::mesh(light));
    }
}

/// The disc in the middle, in the same material.
pub fn core(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    fade: f32,
    tint: Option<Color32>,
    glass: Glass,
) {
    let base = tint.unwrap_or_else(|| glass_colors().core);
    let alpha = (glass.body_alpha() + 0.25).min(1.0);
    painter.circle_filled(center, radius, base.gamma_multiply(alpha * fade));

    // the same lit rim, as a thin ring just inside the edge of the disc
    let band = (radius * 0.45).clamp(3.0, 10.0);
    let look = Look {
        frost: 0.0,
        sheen: 0.0,
        rim: glass.rim_alpha(),
    };
    let veil = veil_color();
    let mesh = field_mesh(
        center,
        (radius - band, radius),
        (0.0, TAU),
        pixel(painter),
        |s| {
            // the distance that counts is the one to the outer edge of the disc
            let from_edge = (1.0 - s.u) * band;
            let s = Sample {
                edge: from_edge,
                ..s
            };
            with_alpha(veil, look.alpha(s) * fade)
        },
    );
    if !mesh.is_empty() {
        painter.add(Shape::mesh(mesh));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass(clear: f32, frost: f32, shine: f32) -> Glass {
        Glass {
            clear,
            frost,
            shine,
        }
    }

    #[test]
    fn a_clearer_glass_is_more_see_through() {
        assert!(glass(0.9, 0.5, 0.5).body_alpha() < glass(0.1, 0.5, 0.5).body_alpha());
    }

    #[test]
    fn the_glass_is_never_fully_clear_and_never_more_than_solid() {
        for c in [0.0, 0.5, 1.0] {
            let a = glass(c, 0.5, 0.5).body_alpha();
            assert!((0.15..=1.0).contains(&a), "{c}: {a}");
        }
        assert_eq!(glass(0.0, 0.0, 0.0).body_alpha(), 1.0);
    }

    #[test]
    fn frost_and_shine_scale_their_layers_and_vanish_at_zero() {
        assert_eq!(glass(0.5, 0.0, 0.5).veil_alpha(), 0.0);
        assert!(glass(0.5, 1.0, 0.5).veil_alpha() > glass(0.5, 0.4, 0.5).veil_alpha());
        assert_eq!(glass(0.5, 0.5, 0.0).sheen_alpha(), 0.0);
        assert!(glass(0.5, 0.5, 1.0).rim_alpha() > glass(0.5, 0.5, 0.0).rim_alpha());
    }

    fn look() -> Look {
        Look {
            frost: 0.26,
            sheen: 0.13,
            rim: 0.6,
        }
    }

    #[test]
    fn the_light_is_continuous_across_the_slice() {
        // the old glass was built from five flat bands; no step may come back
        let mut prev = look().alpha(Sample {
            u: 0.0,
            edge: 20.0,
            theta: LIGHT,
        });
        for k in 1..=200 {
            let a = look().alpha(Sample {
                u: k as f32 / 200.0,
                edge: 20.0,
                theta: LIGHT,
            });
            assert!((a - prev).abs() < 0.01, "step at {k}: {prev} -> {a}");
            prev = a;
        }
        let mut prev = look().alpha(Sample {
            u: 0.5,
            edge: 0.0,
            theta: 0.0,
        });
        for k in 1..=720 {
            let a = look().alpha(Sample {
                u: 0.5,
                edge: 0.0,
                theta: TAU * k as f32 / 720.0,
            });
            assert!((a - prev).abs() < 0.02, "step round the ring at {k}");
            prev = a;
        }
    }

    #[test]
    fn the_rim_is_brightest_toward_the_light_and_fades_away_from_it() {
        let at = lit;
        assert!((at(LIGHT) - 1.0).abs() < 1e-6);
        assert!(at(LIGHT) > at(LIGHT + PI));
        assert!(at(LIGHT + PI) > at(LIGHT + PI / 2.0));
        assert!(at(LIGHT + PI / 2.0) < 1e-4);
        assert!((at(LIGHT) - at(LIGHT + TAU)).abs() < 1e-5);
    }

    #[test]
    fn the_rim_is_a_line_and_the_glow_fades_inward() {
        let at = |edge| {
            look().alpha(Sample {
                u: 0.5,
                edge,
                theta: LIGHT,
            })
        };
        assert!(at(0.0) > at(1.0) && at(1.0) > at(4.0) && at(4.0) > at(12.0));
    }

    #[test]
    fn stops_keep_their_ends_stay_sorted_and_never_leave_a_big_gap() {
        for (lo, hi) in [(10.0, 12.0), (30.0, 140.0), (0.5, 5.0)] {
            let v = stops(lo, hi, 1.0, MAX_GAP);
            assert_eq!(v[0], lo);
            assert_eq!(*v.last().unwrap(), hi);
            for w in v.windows(2) {
                assert!(w[1] > w[0], "{v:?}");
                assert!(w[1] - w[0] <= MAX_GAP + 1e-4, "{v:?}");
            }
        }
    }

    #[test]
    fn field_mesh_is_well_formed_and_its_fringe_is_transparent() {
        let c = Pos2::new(100.0, 100.0);
        let (r0, r1) = (40.0, 120.0);
        let m = field_mesh(c, (r0, r1), (0.2, 1.2), 1.0, |_| Color32::WHITE);
        assert!(!m.is_empty());
        assert_eq!(m.indices.len() % 3, 0);
        assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
        for v in &m.vertices {
            let d = (v.pos - c).length();
            let outside = d < r0 || d > r1;
            if outside {
                assert_eq!(v.color, Color32::TRANSPARENT, "{:?}", v.pos);
            }
        }
        // something is opaque, too
        assert!(m.vertices.iter().any(|v| v.color == Color32::WHITE));
    }

    #[test]
    fn a_full_ring_has_no_sides_and_a_degenerate_slice_has_no_mesh() {
        let c = Pos2::ZERO;
        let ring = field_mesh(c, (30.0, 90.0), (0.0, TAU), 1.0, |_| Color32::WHITE);
        let open = field_mesh(c, (30.0, 90.0), (0.0, 1.0), 1.0, |_| Color32::WHITE);
        assert!(!ring.is_empty() && !open.is_empty());
        // a ring is faded out only at its inner and outer edge; a slice also at its two sides
        let side_fringe = |m: &egui::Mesh| {
            m.vertices
                .iter()
                .filter(|v| v.color == Color32::TRANSPARENT)
                .any(|v| (30.0..=90.0).contains(&v.pos.to_vec2().length()))
        };
        assert!(!side_fringe(&ring));
        assert!(side_fringe(&open));
        assert!(field_mesh(c, (30.0, 30.1), (0.0, 1.0), 1.0, |_| Color32::WHITE).is_empty());
        assert!(field_mesh(c, (30.0, 90.0), (1.0, 1.0), 1.0, |_| Color32::WHITE).is_empty());
    }

    #[test]
    fn the_veil_is_the_pale_grey_of_smoked_glass() {
        assert_eq!(veil_color(), [210, 212, 224]);
    }

    #[test]
    fn only_the_glass_style_gives_glass_and_percentages_are_clamped() {
        let mut c = Config::default();
        assert_eq!(Glass::of(&c), None);
        c.pie_style = crate::config::PieStyle::Glass;
        c.glass_clear = 250;
        let g = Glass::of(&c).expect("glass style");
        assert_eq!(g.clear, 1.0);
    }
}
